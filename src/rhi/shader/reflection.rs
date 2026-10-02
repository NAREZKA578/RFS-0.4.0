//! Shader Reflection
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::descriptor::layout::DescriptorType;
use crate::types::*;

/// Shader reflection
#[derive(Debug, Clone, Default)]
pub struct ShaderReflection {
    pub inputs: Vec<ShaderVariable>,
    pub outputs: Vec<ShaderVariable>,
    pub resources: Vec<ShaderResource>,
    pub push_constants: Vec<ShaderPushConstant>,
}

impl ShaderReflection {
    pub fn new(
        inputs: Vec<ShaderVariable>,
        outputs: Vec<ShaderVariable>,
        resources: Vec<ShaderResource>,
        push_constants: Vec<ShaderPushConstant>,
    ) -> Self {
        Self {
            inputs,
            outputs,
            resources,
            push_constants,
        }
    }

    /// Returns `true` when the reflection contains no data.
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
            && self.outputs.is_empty()
            && self.resources.is_empty()
            && self.push_constants.is_empty()
    }

    /// Finds a resource by descriptor set and binding.
    pub fn find_resource(&self, set: u32, binding: u32) -> Option<&ShaderResource> {
        self.resources
            .iter()
            .find(|r| r.set == set && r.binding == binding)
    }

    /// Returns the total push constant size in bytes.
    pub fn push_constant_size(&self) -> u32 {
        self.push_constants.iter().map(|p| p.size).sum()
    }
}

/// Shader variable type
#[derive(Debug, Clone)]
pub enum ShaderVariableType {
    Scalar(ScalarType),
    Vector(ScalarType, u8),
    Matrix(ScalarType, u8, u8),
    Struct(StructType),
    Array(Box<ShaderVariableType>, u32),
}

/// Scalar type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScalarType {
    Float,
    Int,
    UInt,
    Bool,
    Double,
}

/// Struct type
#[derive(Debug, Clone)]
pub struct StructType {
    pub name: String,
    pub members: Vec<ShaderStructMember>,
}

/// Shader struct member
#[derive(Debug, Clone)]
pub struct ShaderStructMember {
    pub name: String,
    pub ty: ShaderVariableType,
    pub offset: u32,
    pub size: u32,
}

/// Shader variable
#[derive(Debug, Clone)]
pub struct ShaderVariable {
    pub name: String,
    pub location: u32,
    pub ty: ShaderVariableType,
    pub size: u32,
    pub offset: u32,
}

/// Shader resource access
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderResourceAccess {
    Read,
    Write,
    ReadWrite,
}

/// Shader resource dimensions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderResourceDimensions {
    D1,
    D2,
    D3,
    Cube,
    Buffer,
    SubpassData,
}

/// Shader resource
#[derive(Debug, Clone)]
pub struct ShaderResource {
    pub name: String,
    pub binding: u32,
    pub set: u32,
    pub ty: DescriptorType,
    pub access: ShaderResourceAccess,
    pub dimensions: ShaderResourceDimensions,
    pub format: Option<Format>,
}

/// Shader push constant
#[derive(Debug, Clone)]
pub struct ShaderPushConstant {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    pub stages: ShaderStage,
}
