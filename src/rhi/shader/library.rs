//! Shader Library
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use super::module::{ShaderModule, ShaderModuleDesc};

/// Shader library description
#[derive(Debug, Clone, Default)]
pub struct ShaderLibraryDesc {
    pub modules: Vec<ShaderModuleDesc>,
    pub name: String,
}

/// Shader library
pub struct ShaderLibrary {
    desc: ShaderLibraryDesc,
    modules: Vec<ShaderModule>,
}

impl ShaderLibrary {
    pub fn new(desc: ShaderLibraryDesc) -> Self {
        Self {
            desc,
            modules: Vec::new(),
        }
    }

    pub fn get_module(&self, name: &str) -> Option<&ShaderModule> {
        self.modules
            .iter()
            .find(|m| m.desc().name.as_deref() == Some(name))
    }

    pub fn add_module(&mut self, module: ShaderModule) {
        self.modules.push(module);
    }
}
