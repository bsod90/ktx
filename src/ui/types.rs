use std::error::Error;

use crate::ui::views::confirmation::ConfirmationDialogViewState;
use crate::ui::views::import::ImportViewState;
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

// primary id, display name, optional secondary id
#[derive(Debug, Clone)]
pub struct CloudImportPath(Vec<(String, String, Option<String>)>);

pub type EmptyResult = Result<(), Box<dyn Error + Send + Sync>>;

impl CloudImportPath {
    pub fn is_full(&self) -> bool {
        if self.is_empty() {
            false
        } else if self.is_gcp() {
            // GCP path: platform -> project -> cluster
            self.0.len() == 3
        } else if self.is_aws() {
            // AWS path: platform -> profile -> region -> cluster
            self.0.len() == 4
        } else if self.is_azure() {
            // Azure path: platform -> subscription -> cluster
            self.0.len() == 3
        } else {
            false
        }
    }

    pub fn is_listing_clusters(&self) -> bool {
        if self.is_empty() {
            false
        } else if self.is_gcp() {
            self.0.len() == 2
        } else if self.is_aws() {
            self.0.len() == 3
        } else if self.is_azure() {
            self.0.len() == 2
        } else {
            false
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.len() == 0
    }

    pub fn is_aws(&self) -> bool {
        if self.is_empty() {
            return false;
        }
        self.0[0].0 == "aws"
    }

    pub fn is_azure(&self) -> bool {
        if self.is_empty() {
            return false;
        }
        self.0[0].0 == "azure"
    }

    pub fn is_gcp(&self) -> bool {
        if self.is_empty() {
            return false;
        }
        self.0[0].0 == "gcp"
    }

    pub fn has_gcp_project(&self) -> bool {
        self.is_gcp() && self.0.len() > 1
    }

    pub fn get_gcp_project(&self) -> String {
        self.0[1].0.clone()
    }

    pub fn get_gke_zone(&self) -> String {
        self.0[2].2.clone().unwrap()
    }

    pub fn has_aws_profile(&self) -> bool {
        self.is_aws() && self.0.len() > 1
    }

    pub fn get_aws_profile(&self) -> String {
        self.0[1].0.clone()
    }

    pub fn has_azure_subscription(&self) -> bool {
        self.is_azure() && self.0.len() > 1
    }

    pub fn get_azure_subscription(&self) -> String {
        self.0[1].0.clone()
    }

    pub fn get_azure_resource_group(&self) -> String {
        self.0[2].2.clone().unwrap()
    }

    pub fn has_aws_region(&self) -> bool {
        self.is_aws() && self.0.len() > 2
    }

    pub fn get_aws_region(&self) -> String {
        self.0[2].0.clone()
    }

    pub fn get_cluster_id(&self) -> String {
        self.0.last().unwrap().0.clone()
    }

    pub fn push_clone(&self, element: (String, String, Option<String>)) -> Self {
        let mut new_path = self.0.clone();
        new_path.push(element);
        Self(new_path)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn get_platform(&self) -> String {
        self.0[0].0.clone()
    }
}

impl From<Vec<(String, String, Option<String>)>> for CloudImportPath {
    fn from(path: Vec<(String, String, Option<String>)>) -> Self {
        Self(path)
    }
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
    PushErrorMessage(String),
    PushSuccessMessage(String),
    PushInfoMessage(String),
    RefreshConfig,
    SetConnectivityStatus((String, KubeContextStatus)),
    ShowImportView(CloudImportPath),
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
    ImportView(ImportViewState),
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
    ImportViewState => ViewState::ImportView,
);
