use crossterm::event::Event;

#[derive(Clone, Debug)]
pub enum KubeContextStatus {
    Unknown,
    Healthy(String),
    Unhealthy,
}

#[derive(Clone, Debug)]
pub enum RendererMessage {
    Render,
    Stop,
}

#[derive(Debug, Clone)]
pub enum KtxEvent {
    ViewContext(String),
    SetContext(String),
    DeleteContext(String),
    DeleteContextConfirm(String),
    ListSelect(usize),
    DialogConfirm,
    DialogReject,
    ListOneUp,
    ListOneDown,
    ListPageUp,
    ListPageDown,
    ListTop,
    ListBottom,
    SetConnectivityStatus((String, KubeContextStatus)),
    EnterFilterMode,
    ExitFilterMode,
    TestConnections,
    PopView,
    Exit,
    TerminalEvent(Event),
}
