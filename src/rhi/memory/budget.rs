//! Memory Budget
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

/// Memory budget
#[derive(Debug, Clone, Default)]
pub struct MemoryBudget {
    pub total_memory: u64,
    pub used_memory: u64,
    pub free_memory: u64,
    pub buffer_memory: u64,
    pub texture_memory: u64,
    pub pipeline_memory: u64,
    pub descriptor_memory: u64,
}

/// Memory budget manager
pub struct MemoryBudgetManager {
    budget: MemoryBudget,
}

impl Default for MemoryBudgetManager {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBudgetManager {
    pub fn new() -> Self {
        Self {
            budget: MemoryBudget::default(),
        }
    }

    pub fn budget(&self) -> &MemoryBudget {
        &self.budget
    }
}
