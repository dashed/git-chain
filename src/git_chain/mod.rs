use git2::Repository;

pub struct GitChain {
    pub repo: Repository,
    pub executable_name: String,
}

// Re-export impl blocks
mod core;
mod merge;
mod operations;
