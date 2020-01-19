//! Curl agent that executes multiple requests simultaneously.
//!
//! The agent is implemented as a single background thread attached to a
//! "handle". The handle communicates with the agent thread by using message
//! passing. The agent executes multiple curl requests simultaneously by using a
//! single "multi" handle.
//!
//! Since request executions are driven through futures, the agent also acts as
//! a specialized task executor for tasks related to requests.

use crate::handler::RequestHandler;
use crate::task::{UdpWaker, WakerExt};
use crate::Error;
use crossbeam_channel::{Receiver, Sender};
use crossbeam_utils::sync::WaitGroup;
use curl::multi::{WaitFd, Easy2Handle};
use slab::Slab;
use std::net::{UdpSocket, TcpListener};
use std::sync::{Arc, Mutex};
use std::task::Waker;
use std::thread;
use std::time::{Duration, Instant};
use futures_util::task::{Context, Poll};
use std::pin::Pin;
use futures_util::future::Future;

type EasyHandle = curl::easy::Easy2<RequestHandler>;
type MultiMessage = (usize, Result<(), curl::Error>);

/// Builder for configuring and spawning an agent.
#[derive(Debug, Default)]
pub(crate) struct AgentBuilder {
    max_connections: usize,
    max_connections_per_host: usize,
    connection_cache_size: usize,
}

impl AgentBuilder {
    pub(crate) fn max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    pub(crate) fn max_connections_per_host(mut self, max: usize) -> Self {
        self.max_connections_per_host = max;
        self
    }

    pub(crate) fn connection_cache_size(mut self, size: usize) -> Self {
        self.connection_cache_size = size;
        self
    }

    /// Spawn a new agent using the configuration in this builder and return a
    /// handle for communicating with the agent.
    pub(crate) fn spawn(&self) -> Result<Handle, Error> {
        let max_connections = self.max_connections;
        let max_connections_per_host = self.max_connections_per_host;
        let connection_cache_size = self.connection_cache_size;

        let mut multi = curl::multi::Multi::new();


        if self.max_connections > 0 {
            multi.set_max_total_connections(self.max_connections)?;
        }

        if self.max_connections_per_host > 0 {
            multi.set_max_host_connections(self.max_connections_per_host)?;
        }

        // Only set maxconnects if greater than 0, because 0 actually means unlimited.
        if self.connection_cache_size > 0 {
            multi.set_max_connects(self.connection_cache_size)?;
        }

        let handle = Handle {
            multi
        };

        Ok(handle)
    }
}

/// A handle which executes requests in the current thread.
///
/// Dropping the handle will cause the agent thread to shut down and abort any
/// pending transfers.
#[derive(Debug)]
pub(crate) struct Handle {
    pub multi: curl::multi::Multi,
}


impl Handle {
    /// Begin executing a request with this agent.
    pub(crate) fn submit_request(&self, mut request: EasyHandle) -> Result<impl Future<Output=Result<Option<Easy2Handle<RequestHandler>>, curl::MultiError>> + '_, Error> {
        Ok(RequestFuture {
            agent: self,
            request: Some(request),
            handle: None,
        })
    }

    pub(crate) fn perform_and_send_result_to_handle(&self, handle: &mut Option<Easy2Handle<RequestHandler>>) -> Poll<Result<(), curl::MultiError>> {
        match self.multi.perform() {
            Ok(0) => {
                if let Some(handle) = handle.take() {
                    let mut result_from_curl = None;
                    self.multi.messages(|message| {
                        if let Some(result) = message.result() {
                            if let Ok(token) = message.token() {
                                result_from_curl = Some(result);
                            }
                        }
                    });

                    let mut request = self.multi.remove2(handle)?;
                    if let Some(result_from_curl) = result_from_curl {
                        request.get_mut().on_result(result_from_curl);
                    }
                    request.get_mut().on_result(Ok(()));
                }

                Poll::Ready(Ok(()))
            }
            Ok(_) => {
                Poll::Pending
            }
            Err(e) => {
                if let Some(handle) = handle.take() {
                    let mut request = self.multi.remove2(handle)?;
                    request.get_mut().on_result(Err(curl::Error::new(e.code() as  _)));
                }

                Poll::Ready(Err(e))
            }
        }
    }
}

struct RequestFuture<'a> {
    agent: &'a Handle,
    request: Option<EasyHandle>,
    handle: Option<Easy2Handle<RequestHandler>>,
}

#[allow(unsafe_code)]
unsafe impl Send for RequestFuture<'_> {}

impl Future for RequestFuture<'_> {
    type Output = Result<Option<Easy2Handle<RequestHandler>>, curl::MultiError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(mut request) = self.request.take() {
            let raw_request = request.raw();
            request.get_mut().init(
                0,
                raw_request,
                cx.waker().clone(),
                cx.waker().clone(),
            );

            self.handle = Some(self.agent.multi.add2(request)?);

            // TODO: Remove this.
            cx.waker().clone().wake();

            return Poll::Pending;
        }

        match self.agent.perform_and_send_result_to_handle(&mut self.handle) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(self.handle.take())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => {
                if self.handle.as_ref()
                    .map(|handle| handle.get_ref().is_complete()).unwrap_or(false) {
                    Poll::Ready(Ok(self.handle.take()))
                } else {
                    cx.waker().clone().wake();
                    Poll::Pending
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_send<T: Send>() {}

    fn is_sync<T: Sync>() {}

    #[test]
    fn traits() {
        //is_send::<Handle>();
        //is_sync::<Handle>();
    }
}
