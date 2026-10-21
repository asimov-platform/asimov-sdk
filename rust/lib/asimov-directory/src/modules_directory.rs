// This is free and unencumbered software released into the public domain.

/// A module directory in the abstract.
pub trait ModulesDirectory {
    /// Checks if a module is installed.
    fn is_installed(&self, _module_name: impl AsRef<str>) -> bool {
        false
    }

    /// Checks if a module is installed and enabled.
    fn is_enabled(&self, _module_name: impl AsRef<str>) -> bool {
        false
    }
}
