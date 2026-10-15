//! FileNav — ядро: состояние, шина команд, runtime.
//!
//! Владелец — S1. API — specs/S1.md §2 с изменениями S0-contracts-v2…v5.

#![forbid(unsafe_code)]

pub mod cancel;
pub mod cmd;
pub mod entry;
pub mod ids;
pub mod loc;
pub mod mask;
pub mod msg;
pub mod runtime;

pub use cancel::{CancelToken, Cancelled};
pub use cmd::{
    ArgSpec, Args, CmdError, Command, CommandId, CommandMeta, CommandRegistry, Origin, SideRef,
    Value, ValueKind,
};
pub use entry::{Attrs, Entry, EntryKind};
pub use ids::{Gen, JobId, PromptId, Side, TabId};
pub use loc::{LocError, Location};
pub use mask::{Mask, MaskError};
pub use msg::{Cx, Msg, MsgInbox, MsgSink, SendError, TrySendError, Waker, msg};
pub use runtime::{Lane, Runtime, RuntimeConfig, TaskCx};

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_smoke() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
        assert_ne!(super::version(), "");
    }
}
