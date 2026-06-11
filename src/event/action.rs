use crate::app::ActiveView;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    RequestQuit,
    CancelQuit,
    SwitchTab(ActiveView),
    NextTab,
    PrevTab,
    SelectNext,
    SelectPrev,
    SelectFirst,
    SelectLast,
    Confirm,
    TogglePreview,
    CycleSort,
    FilterStale,
    ClearFilter,
    ResumeSession,
    Refresh,
    Tick,
    #[allow(dead_code)]
    DataRefreshed,
    CloseOverlay,
    ToggleHelp,
    // Search
    EnterSearch,
    ExitSearch,
    SearchInput(char),
    SearchBackspace,
    // Clipboard
    CopySessionId,
    // Editor
    OpenInEditor,
    // Agent filter
    CycleAgentFilter,
    // Review
    CycleReviewRange,
    CycleReviewRangeBack,
    ToggleReviewExpand,
    // Pane interaction (approve/deny permission prompts, write text to agent)
    ApprovePermission,
    DenyPermission,
    EnterPrompt,
    PromptInput(char),
    PromptBackspace,
    SubmitPrompt,
    CancelPrompt,
    None,
}
