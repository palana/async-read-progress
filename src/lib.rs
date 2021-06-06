//! Report the progress of an async read operation
//!
//! As promised [on Twitter](https://twitter.com/killercup/status/1254695847796842498).
//!
//! # Examples
//!
//! ```
//! # fn main() {
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! use futures::{
//!     io::AsyncReadExt,
//!     stream::{self, TryStreamExt},
//! };
//! use async_read_progress::*;
//!
//! let src = vec![1u8, 2, 3, 4, 5];
//! let total_size = src.len();
//! let reader = stream::iter(vec![Ok(src)]).into_async_read();
//!
//! let mut reader = reader.report_progress(
//!     /* only call every */ std::time::Duration::from_millis(20),
//!     |bytes_read| eprintln!("read {}/{}", bytes_read, total_size),
//! );
//! #
//! # let mut buf = Vec::new();
//! # assert!(reader.read_to_end(&mut buf).await.is_ok());
//! # });
//! # }
//! ```
use core::pin::Pin;
use std::{
    fmt,
    time::{Duration, Instant},
};

pub use for_futures::FReportReadProgress as AsyncReadProgressExt;

#[cfg(feature = "with-tokio")]
pub use for_tokio::TReportReadProgress as TokioAsyncReadProgressExt;
#[cfg(feature = "with-tokio")]
pub use for_tokio::TReportWriteProgress as TokioAsyncWriteProgressExt;

/// Reader for the `report_progress` method.
#[must_use = "streams do nothing unless polled"]
pub struct LogStreamProgress<St, F> {
    inner: St,
    callback: F,
    state: State,
}

struct State {
    bytes_processed: usize,
    // TODO: Actually use this
    at_most_ever: Duration,
    last_call_at: Instant,
}

// TODO: Remove this comment after someone who knows how this actually works has
// reviewed/fixed this.
impl<St, F: FnMut(usize)> LogStreamProgress<St, F> {
    pin_utils::unsafe_pinned!(inner: St);
    pin_utils::unsafe_unpinned!(callback: F);
    pin_utils::unsafe_unpinned!(state: State);

    fn update(mut self: Pin<&mut Self>, bytes_processed: usize) {
        let mut state = self.as_mut().state();
        state.bytes_processed += bytes_processed;
        let processed = state.bytes_processed;

        if state.last_call_at.elapsed() >= state.at_most_ever {
            (self.as_mut().callback())(processed);

            self.as_mut().state().last_call_at = Instant::now();
        }
    }
}

impl<T, U> Unpin for LogStreamProgress<T, U>
where
    T: Unpin,
    U: Unpin,
{
}

impl<St, F> fmt::Debug for LogStreamProgress<St, F>
where
    St: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogStreamProgress")
            .field("stream", &self.inner)
            .field("at_most_ever", &self.state.at_most_ever)
            .field("last_call_at", &self.state.last_call_at)
            .finish()
    }
}

mod for_futures {
    use core::{
        pin::Pin,
        task::{Context, Poll},
    };
    use futures_io::{AsyncRead as FAsyncRead, IoSliceMut};
    use std::{
        io,
        time::{Duration, Instant},
    };

    /// An extension trait which adds the `report_progress` method to
    /// `AsyncRead` types.
    ///
    /// Note: This is for [`futures_io::AsyncRead`].
    pub trait FReportReadProgress {
        fn report_progress<F>(
            self,
            at_most_ever: Duration,
            callback: F,
        ) -> super::LogStreamProgress<Self, F>
        where
            Self: Sized,
            F: FnMut(usize),
        {
            let state = super::State {
                bytes_processed: 0,
                at_most_ever,
                last_call_at: Instant::now(),
            };
            super::LogStreamProgress {
                inner: self,
                callback,
                state,
            }
        }
    }

    impl<R: FAsyncRead + ?Sized> FReportReadProgress for R {}

    impl<'a, St, F> FAsyncRead for super::LogStreamProgress<St, F>
    where
        St: FAsyncRead,
        F: FnMut(usize),
    {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            match self.as_mut().inner().poll_read(cx, buf) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Ready(Ok(bytes_read)) => {
                    self.update(bytes_read);
                    Poll::Ready(Ok(bytes_read))
                }
            }
        }

        fn poll_read_vectored(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bufs: &mut [IoSliceMut<'_>],
        ) -> Poll<io::Result<usize>> {
            match self.as_mut().inner().poll_read_vectored(cx, bufs) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Ready(Ok(bytes_read)) => {
                    self.update(bytes_read);
                    Poll::Ready(Ok(bytes_read))
                }
            }
        }
    }
}

#[cfg(feature = "with-tokio")]
mod for_tokio {
    use core::{
        pin::Pin,
        task::{Context, Poll},
    };
    use std::{
        io,
        time::{Duration, Instant},
    };
    use tokio::io::{AsyncRead as TAsyncRead, AsyncWrite as TAsyncWrite, ReadBuf};

    /// An extension trait which adds the `report_progress` method to
    /// `AsyncRead` types.
    ///
    /// Note: This is for [`tokio::io::AsyncRead`].
    pub trait TReportReadProgress {
        fn report_progress<F>(
            self,
            at_most_ever: Duration,
            callback: F,
        ) -> super::LogStreamProgress<Self, F>
        where
            Self: Sized,
            F: FnMut(usize),
        {
            let state = super::State {
                bytes_processed: 0,
                at_most_ever,
                last_call_at: Instant::now(),
            };
            super::LogStreamProgress {
                inner: self,
                callback,
                state,
            }
        }
    }

    impl<R: TAsyncRead + ?Sized> TReportReadProgress for R {}

    impl<'a, St, F> TAsyncRead for super::LogStreamProgress<St, F>
    where
        St: TAsyncRead,
        F: FnMut(usize),
    {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf,
        ) -> Poll<io::Result<()>> {
            match self.as_mut().inner().poll_read(cx, buf) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Ready(Ok(())) => {
                    self.update(buf.filled().len());
                    Poll::Ready(Ok(()))
                }
            }
        }
    }

    /// An extension trait which adds the `report_progress` method to
    /// `AsyncWrite` types.
    ///
    /// Note: This is for [`tokio::io::AsyncWrite`].
    pub trait TReportWriteProgress {
        fn report_progress<F>(
            self,
            at_most_ever: Duration,
            callback: F,
        ) -> super::LogStreamProgress<Self, F>
        where
            Self: Sized,
            F: FnMut(usize),
        {
            let state = super::State {
                bytes_processed: 0,
                at_most_ever,
                last_call_at: Instant::now(),
            };
            super::LogStreamProgress {
                inner: self,
                callback,
                state,
            }
        }
    }

    impl<R: TAsyncWrite + ?Sized> TReportWriteProgress for R {}

    impl<'a, St, F> TAsyncWrite for super::LogStreamProgress<St, F>
    where
        St: TAsyncWrite,
        F: FnMut(usize),
    {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, io::Error>> {
            match self.as_mut().inner().poll_write(cx, buf) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Ready(Ok(bytes_written)) => {
                    self.update(bytes_written);
                    Poll::Ready(Ok(bytes_written))
                }
            }
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
            self.as_mut().inner().poll_flush(cx)
        }

        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
            self.as_mut().inner().poll_shutdown(cx)
        }
    }

    #[test]
    fn works_with_tokios_async_read() {
        use tokio::io::AsyncReadExt;

        let src = &[1u8, 2, 3, 4, 5];
        let total_size = src.len();
        let mut buf = Vec::new();

        let mut reader = src.report_progress(
            /* only call every */ Duration::from_millis(20),
            |bytes_read| eprintln!("read {}/{}", bytes_read, total_size),
        );

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            assert!(reader.read_to_end(&mut buf).await.is_ok());
        });
    }

    #[test]
    fn works_with_tokios_async_write() {
        use tokio::io::{AsyncWriteExt, sink};

        let src = &[1u8, 2, 3, 4, 5];
        let total_size = src.len();

        let mut writer = sink().report_progress(
            /* only call every */ Duration::from_millis(20),
            |bytes_written| eprintln!("written {}/{}", bytes_written, total_size),
        );

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            assert!(writer.write(src).await.is_ok());
        });
    }

    // Disabled for now since Tokio 1.6 mpsc channels don't implement AsyncRead
    /*
    #[tokio::test]
    async fn does_delays_and_stuff() {
        use bytes::Bytes;
        use std::sync::{Arc, RwLock};
        use tokio::{
            io::AsyncReadExt,
            sync::mpsc,
            time::delay_for,
        };

        let (mut data_writer, data_reader) = mpsc::channel(1);

        tokio::spawn(async move {
            for i in 0u8..10 {
                dbg!(i);
                data_writer
                    .send(Ok(Bytes::from_static(&[1u8, 2, 3, 4])))
                    .await
                    .unwrap();
                delay_for(Duration::from_millis(10)).await;
            }
            drop(data_writer);
        });

        let total_size = 4 * 10i32;
        let mut buf = Vec::new();

        let log = Arc::new(RwLock::new(Vec::new()));
        let log_writer = log.clone();

        let mut reader = data_reader.report_progress(
            /* only call every */ Duration::from_millis(10),
            |bytes_read| {
                log_writer.write().unwrap().push(format!(
                    "read {}/{}",
                    dbg!(bytes_read),
                    total_size
                ));
            },
        );

        assert!(reader.read_to_end(&mut buf).await.is_ok());
        dbg!("read it");

        let log = log.read().unwrap();
        assert_eq!(
            *log,
            &[
                "read 8/40".to_string(),
                "read 12/40".to_string(),
                "read 16/40".to_string(),
                "read 20/40".to_string(),
                "read 24/40".to_string(),
                "read 28/40".to_string(),
                "read 32/40".to_string(),
                "read 36/40".to_string(),
                "read 40/40".to_string(),
                "read 40/40".to_string(),
            ]
        );
    }
    */

    // Disabled for now since Tokio 1.6 mpsc channels don't implement AsyncRead
    /*
    #[tokio::test]
    async fn does_delays_and_stuff_real_good() {
        use bytes::Bytes;
        use std::sync::{Arc, RwLock};
        use tokio::{
            io::{stream_reader, AsyncReadExt},
            sync::mpsc,
            time::delay_for,
        };

        let (mut data_writer, data_reader) = mpsc::channel(1);

        tokio::spawn(async move {
            for i in 0u8..10 {
                dbg!(i);
                data_writer
                    .send(Ok(Bytes::from_static(&[1u8, 2, 3, 4])))
                    .await
                    .unwrap();
                delay_for(Duration::from_millis(5)).await;
            }
            drop(data_writer);
        });

        let total_size = 4 * 10i32;
        let reader = stream_reader(data_reader);
        let mut buf = Vec::new();

        let log = Arc::new(RwLock::new(Vec::new()));
        let log_writer = log.clone();

        let mut reader = reader.report_progress(
            /* only call every */ Duration::from_millis(10),
            |bytes_read| {
                log_writer.write().unwrap().push(format!(
                    "read {}/{}",
                    dbg!(bytes_read),
                    total_size
                ));
            },
        );

        assert!(reader.read_to_end(&mut buf).await.is_ok());
        dbg!("read it");

        let log = log.read().unwrap();
        assert_eq!(
            *log,
            &[
                "read 12/40".to_string(),
                "read 20/40".to_string(),
                "read 28/40".to_string(),
                "read 36/40".to_string(),
                "read 40/40".to_string(),
            ]
        );
    }
    */
}
