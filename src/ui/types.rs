use crate::ui::views::confirmation::ConfirmationDialogViewState;
use crate::ui::views::list::ContextListViewState;
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
    // ViewContext(String),
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

pub enum ViewState {
    ContextListView(ContextListViewState),
    ConfirmationDialogView(ConfirmationDialogViewState),
}

macro_rules! impl_view_state {
    ($($state:ty => $variant:path),* $(,)?) => {
        $(
            impl $state {
                pub fn from_view_state(state: &mut ViewState) -> &mut Self {
                    if let $variant(state) = state {
                        state
                    } else {
                        panic!(concat!("Invalid ViewState passed to ", stringify!($state)))
                    }
                }
            }
        )*
    };
}

// usage
impl_view_state!(
    ConfirmationDialogViewState => ViewState::ConfirmationDialogView,
    ContextListViewState => ViewState::ContextListView,
);
