/// Resolve a task capability to the event kind used to dispatch it.
///
/// In M3 the capability *is* the event kind. This function is the seam where a
/// future version can rank multiple candidate plugins (by `PluginMetadata`)
/// for the same capability and pick the best one.
pub fn resolve(capability: &str) -> String {
    capability.to_string()
}
