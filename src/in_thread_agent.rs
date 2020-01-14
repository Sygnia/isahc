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
use curl::multi::WaitFd;
use slab::Slab;
use std::net::{UdpSocket, TcpListener};
use std::sync::{Arc, Mutex};
use std::task::Waker;
use std::thread;
use std::time::{Duration, Instant};
use futures_util::task::{Context, Poll};
use std::pin::Pin;

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

        let handle = Handle {
            max_connections,
            max_connections_per_host,
            connection_cache_size,
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
    max_connections: usize,
    max_connections_per_host: usize,
    connection_cache_size: usize,
}


impl Handle {
    /// Begin executing a request with this agent.
    pub(crate) fn submit_request(&self, mut request: EasyHandle) -> Result<(), Error> {
        let mut multi = curl::multi::Multi::new();

        // TODO: Remove this.
        // Create an UDP socket for the agent thread to listen for wakeups on.
        let wake_socket = TcpListener::bind("127.0.0.1:0")?;
        wake_socket.set_nonblocking(true)?;
        let wake_addr = wake_socket.local_addr()?;
        let waker = futures_util::task::waker(Arc::new(UdpWaker::connect(wake_addr)?));

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

        let raw = request.raw();

        request.get_mut().init(0, raw, waker.clone(),
                               waker.clone());

        let handle = multi.add2(request)?;

        while multi.perform()? != 0 {

        }

        let mut request = multi.remove2(handle)?;

        request.get_mut().on_result(Ok(()));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_send<T: Send>() {}

    fn is_sync<T: Sync>() {}

    #[test]
    fn traits() {
        is_send::<Handle>();
        is_sync::<Handle>();
    }
}
