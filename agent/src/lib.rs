pub mod provider;
pub mod session;

use provider::Provider;

pub struct Agent<P: Provider> {
    provider: P,
}

impl<P: Provider> Agent<P> {
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
}
