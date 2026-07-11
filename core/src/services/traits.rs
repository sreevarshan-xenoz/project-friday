use async_trait::async_trait;

use crate::error::Result;

/// A long-lived, addressable capability hosted by the runtime. Services differ
/// from [`crate::plugin::Plugin`] in that they expose a stable identity and
/// lifecycle (`start`/`stop`) and are reachable via the protocol transport.
/// A plugin typically hosts one or more services.
#[async_trait]
pub trait Service: Send + Sync {
    fn id(&self) -> &str;

    async fn start(&mut self) -> Result<()> {
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}
