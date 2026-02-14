// Merge options types
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SquashedMergeHandling {
    // Reset the branch to the parent branch
    Reset,

    // Skip merging the branch
    Skip,

    // Force a merge despite the squashed merge detection
    Merge,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SquashedRebaseHandling {
    // Reset the branch to the parent branch (with auto-backup)
    Reset,

    // Skip the squashed branch during rebase
    Skip,

    // Force normal rebase despite squash detection
    Rebase,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ReportLevel {
    // Minimal reporting (just success/failure)
    Minimal,

    // Standard reporting (summary with counts)
    Standard,

    // Detailed reporting (all actions and their results)
    Detailed,
}

pub enum MergeResult {
    // Successfully merged with changes
    Success(String), // Contains the merge output message

    // Already up-to-date, no changes needed
    AlreadyUpToDate,

    // Merge conflict occurred
    Conflict(String), // Contains the conflict message
}

pub struct MergeOptions {
    // Skip the merge of the root branch into the first branch
    pub ignore_root: bool,

    // Git merge options passed to all merge operations
    pub merge_flags: Vec<String>,

    // Whether to use fork point detection (more accurate but slower)
    pub use_fork_point: bool,

    // How to handle squashed merges (reset, skip, merge)
    pub squashed_merge_handling: SquashedMergeHandling,

    // Print verbose output
    pub verbose: bool,

    // Return to original branch after merging
    pub return_to_original: bool,

    // Use simple merge mode
    pub simple_mode: bool,

    // Level of detail in the final report
    pub report_level: ReportLevel,
}

impl Default for MergeOptions {
    fn default() -> Self {
        MergeOptions {
            ignore_root: false,
            merge_flags: vec![],
            use_fork_point: true,
            squashed_merge_handling: SquashedMergeHandling::Reset,
            verbose: false,
            return_to_original: true,
            simple_mode: false,
            report_level: ReportLevel::Standard,
        }
    }
}

pub enum BranchSearchResult {
    NotPartOfAnyChain,
    Branch(crate::Branch),
}

pub enum SortBranch {
    First,
    Last,
    Before(crate::Branch),
    After(crate::Branch),
}

// Structure to hold merge commit information
#[derive(Debug)]
pub struct MergeCommitInfo {
    pub message: Option<String>,
    pub stats: Option<MergeStats>,
}

#[derive(Debug)]
pub struct MergeStats {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}
