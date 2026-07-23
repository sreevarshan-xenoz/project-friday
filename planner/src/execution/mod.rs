pub mod capability_registry;
pub mod dispatcher;
pub mod executor;
pub mod lifecycle;
pub mod native_caps;
pub mod persistence;
pub mod plugin_selector;
pub mod reflection;
pub mod scheduler;
pub mod supervisor;

pub use capability_registry::{
    CapabilityHealth, CapabilityRegistry, CapabilitySource, CapabilitySpec, ExecutionBinding, Risk,
};
pub use dispatcher::dispatch_event;
pub use executor::ExecutionEngine;
pub use lifecycle::{ContextEntry, ExecutionBudget, GoalContext, GoalRuntime};
pub use persistence::{GoalEvent, GoalStore};
pub use plugin_selector::resolve;
pub use reflection::{Reflector, ReflectionAction, RuleReflector};
pub use scheduler::schedule_batch;
pub use supervisor::{GoalSupervisor, PolicyAuthority};
