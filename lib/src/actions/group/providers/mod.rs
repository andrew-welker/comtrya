// mod freebsd;
// use self::freebsd::FreeBSDUserProvider;
use crate::steps::Step;
mod none;
use self::{freebsd::FreeBSDGroupProvider, none::NoneGroupProvider};
use super::GroupVariant;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
mod freebsd;
mod linux;
use self::linux::LinuxGroupProvider;

#[derive(JsonSchema, Clone, Debug, Serialize, Deserialize)]
pub enum GroupProviders {
    #[serde(alias = "none")]
    None,

    #[serde(alias = "freebsd")]
    FreeBSD,

    #[serde(alias = "linux")]
    Linux,
}

impl GroupProviders {
    pub fn get_provider(self) -> Box<dyn GroupProvider> {
        match self {
            GroupProviders::None => Box::new(NoneGroupProvider {}),
            GroupProviders::FreeBSD => Box::new(FreeBSDGroupProvider {}),
            GroupProviders::Linux => Box::new(LinuxGroupProvider {}),
        }
    }
}

impl Default for GroupProviders {
    #[cfg(target_os = "linux")]
    fn default() -> Self {
        GroupProviders::Linux
    }

    #[cfg(not(target_os = "linux"))]
    fn default() -> Self {
        let info = os_info::get();

        match info.os_type() {
            os_info::Type::FreeBSD => GroupProviders::FreeBSD,
            _ => GroupProviders::None,
        }
    }
}

pub trait GroupProvider {
    fn add_group(&self, group: &GroupVariant) -> Vec<Step>;
}
