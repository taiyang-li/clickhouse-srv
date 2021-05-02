use std::sync::Arc;

use chrono_tz::Tz;
use errors::Result;
use log::debug;
use log::error;
use tokio::net::TcpStream;
use tokio_stream::StreamExt;

use crate::types::Block;
use crate::types::Progress;
use crate::connection::Connection;
use tokio::sync::broadcast;
use crate::cmd::Cmd;
use crate::protocols::HelloRequest;

mod binary;
pub mod error_codes;
pub mod errors;
pub mod protocols;
pub mod types;
pub mod connection;
pub mod cmd;

#[macro_use]
extern crate bitflags;

#[async_trait::async_trait]
pub trait ClickHouseSession: Send + Sync {
    async fn execute_query(&self, ctx: &mut CHContext, connection: &mut Connection) -> Result<()>;

    fn with_stack_trace(&self) -> bool {
        false
    }

    fn dbms_name(&self) -> &str {
        "clickhouse-server"
    }

    // None is by default, which will use same version as client send
    fn dbms_version_major(&self) -> u64 {
        19
    }

    fn dbms_version_minor(&self) -> u64 {
        17
    }

    fn dbms_tcp_protocol_version(&self) -> u64 {
        54428
    }

    fn timezone(&self) -> &str {
        "UTC"
    }

    fn server_display_name(&self) -> &str {
        "clickhouse-server"
    }

    fn dbms_version_patch(&self) -> u64 {
        1
    }

    fn get_progress(&self) -> Progress {
        Progress::default()
    }
}

#[derive(Default, Clone)]
pub struct QueryState {
    pub query_id: String,
    pub stage: u64,
    pub compression: u64,
    pub query: String,
    pub is_cancelled: bool,
    pub is_connection_closed: bool,
    /// empty or not
    pub is_empty: bool,
    /// Data was sent.
    pub sent_all_data: bool,
}

impl QueryState {
    fn reset(&mut self) {
        self.stage = 0;
        self.is_cancelled = false;
        self.is_connection_closed = false;
        self.is_empty = false;
        self.sent_all_data = false;
    }
}

#[derive(Clone)]
pub struct CHContext {
    pub state: QueryState,

    pub client_revision: u64,
    pub hello: Option<HelloRequest>,
}

impl CHContext {
    fn new(state: QueryState) -> Self {
        Self { state, client_revision: 0, hello: None }
    }
}

/// A server that speaks the ClickHouseprotocol, and can delegate client commands to a backend
/// that implements [`ClickHouseSession`]
pub struct ClickHouseServer {}

impl ClickHouseServer {
    pub async fn run_on_stream(
        session: Arc<dyn ClickHouseSession>,
        stream: TcpStream,
    ) -> Result<()> {
        ClickHouseServer::run_on(session, stream.into()).await
    }
}

impl ClickHouseServer {
    async fn run_on(session: Arc<dyn ClickHouseSession>, stream: TcpStream) -> Result<()> {
        let mut srv = ClickHouseServer {};
        srv.run(session, stream).await?;
        Ok(())
    }

    async fn run(&mut self, session: Arc<dyn ClickHouseSession>, stream: TcpStream) -> Result<()> {
        debug!("Handle New session");
        let tz: Tz = session.timezone().parse()?;
        let mut ctx = CHContext::new(QueryState::default());
        let mut connection = Connection::new(stream, session, tz);

        loop {
            // signal.
            let maybe_packet = tokio::select! {
               res = connection.read_packet(&mut ctx) => res,
            };

            let packet = match maybe_packet? {
                Some(packet) => packet,
                None => return Ok(()),
            };
            let mut cmd = Cmd::create(packet);
            cmd.apply(&mut connection, &mut ctx).await?;
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! row {
    () => { $crate::types::RNil };
    ( $i:ident, $($tail:tt)* ) => {
        row!( $($tail)* ).put(stringify!($i).into(), $i.into())
    };
    ( $i:ident ) => { row!($i: $i) };

    ( $k:ident: $v:expr ) => {
        $crate::types::RNil.put(stringify!($k).into(), $v.into())
    };

    ( $k:ident: $v:expr, $($tail:tt)* ) => {
        row!( $($tail)* ).put(stringify!($k).into(), $v.into())
    };

    ( $k:expr => $v:expr ) => {
        $crate::types::RNil.put($k.into(), $v.into())
    };

    ( $k:expr => $v:expr, $($tail:tt)* ) => {
        row!( $($tail)* ).put($k.into(), $v.into())
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
