use std::{error::Error, sync::Arc};

use async_trait::async_trait;
use crossterm::event::{Event, KeyCode, KeyEvent};
use tokio::sync::{mpsc, Mutex};
use tui::{
    backend::Backend,
    layout::Rect,
    text::Line,
    widgets::{ListItem, ListState, Paragraph},
    Frame,
};

use crate::ui::{
    app::AppState,
    types::{CloudImportPath, KtxEvent, ViewState},
    AppView,
};

use super::ui_utils::{
    action_style, handle_list_navigation_event, handle_list_navigation_keyboard_event, key_style,
    styled_list,
};

pub struct ImportViewState {
    pub list_state: ListState,
    pub remembered_g: bool,
    pub options: Vec<(String, String)>,
}

pub struct ImportView {
    event_bus_tx: mpsc::Sender<KtxEvent>,
    state: Arc<Mutex<ViewState>>,
    import_path: CloudImportPath,
}

async fn exec_to_str(cmd: &str, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await?;
    let output = String::from_utf8_lossy(&output.stdout);
    Ok(output.to_string())
}

async fn exec_to_json(cmd: &str, args: &[&str]) -> Result<serde_json::Value, Box<dyn Error>> {
    let output = exec_to_str(cmd, args).await?;
    let json: serde_json::Value = serde_json::from_str(&output)?;
    Ok(json)
}

impl ImportView {
    pub fn new<B: Backend>(
        event_bus_tx: mpsc::Sender<KtxEvent>,
        import_path: CloudImportPath,
    ) -> Self {
        let mut state = ImportViewState {
            list_state: ListState::default(),
            remembered_g: false,
            options: vec![],
        };
        state.list_state.select(Some(0));
        Self {
            event_bus_tx,
            import_path,
            state: Arc::new(Mutex::new(ViewState::ImportView(state))),
        }
    }

    async fn is_gcp_configured(&self) -> bool {
        match exec_to_json("gcloud", &["--format", "json", "info"]).await {
            Err(_) => return false,
            Ok(info) => {
                let account = info["config"]["account"].as_str().unwrap_or("");
                return !account.is_empty();
            }
        }
    }

    async fn is_aws_configured(&self) -> bool {
        match exec_to_str("aws", &["configure", "list-profiles"]).await {
            Err(_) => return false,
            Ok(output) => {
                let profiles = output.split("\n").collect::<Vec<&str>>();
                return !profiles.is_empty();
            }
        };
    }

    async fn is_azure_configured(&self) -> bool {
        match exec_to_json("az", &["account", "show", "--output", "json"]).await {
            Err(_) => return false,
            Ok(account) => {
                let user = account["user"]["name"].as_str().unwrap_or("");
                return !user.is_empty();
            }
        };
    }

    async fn load_cloud_options(&self, state: &mut ImportViewState) {
        let (gcp_configured, aws_configured, azure_configured) = tokio::join!(
            self.is_gcp_configured(),
            self.is_aws_configured(),
            self.is_azure_configured()
        );
        if aws_configured {
            state.options.push(("aws".to_string(), "AWS".to_string()));
        }
        if gcp_configured {
            state.options.push(("gcp".to_string(), "GCP".to_string()));
        }
        if azure_configured {
            state
                .options
                .push(("azure".to_string(), "MS Azure".to_string()));
        };
    }

    async fn load_gcp_projects(&self, state: &mut ImportViewState) {
        match exec_to_json("gcloud", &["--format", "json", "projects", "list"]).await {
            Err(_) => return,
            Ok(projects) => {
                for project in projects.as_array().unwrap() {
                    let project_id = project["projectId"].as_str().unwrap_or("");
                    let project_name = project["name"].as_str().unwrap_or("");
                    let lifecycle_state = project["lifecycleState"].as_str().unwrap_or("");
                    if !project_id.is_empty()
                        && !project_id.starts_with("sys-")
                        && !project_name.is_empty()
                        && lifecycle_state == "ACTIVE"
                    {
                        state
                            .options
                            .push((project_id.to_string(), project_name.to_string()));
                    }
                }
            }
        };
    }

    async fn load_gke_clusters(&self, state: &mut ImportViewState, project: &str) {
        match exec_to_json(
            "gcloud",
            &[
                "--format",
                "json",
                "container",
                "clusters",
                "list",
                "--project",
                project,
            ],
        )
        .await
        {
            Err(_) => return,
            Ok(clusters) => {
                for cluster in clusters.as_array().unwrap() {
                    let cluster_name = cluster["name"].as_str().unwrap_or("");
                    let cluster_id = cluster["id"].as_str().unwrap_or("");
                    state
                        .options
                        .push((cluster_id.to_string(), cluster_name.to_string()));
                }
            }
        };
    }

    async fn load_aws_profiles(&self, state: &mut ImportViewState) {
        match exec_to_str("aws", &["configure", "list-profiles"]).await {
            Err(_) => return,
            Ok(output) => {
                let profiles = output.split("\n").collect::<Vec<&str>>();
                for profile in profiles {
                    if !profile.is_empty() {
                        state
                            .options
                            .push((profile.to_string(), profile.to_string()));
                    }
                }
            }
        };
    }

    async fn load_aws_regions(&self, state: &mut ImportViewState, profile: &str) {
        match exec_to_json(
            "aws",
            &[
                "--profile",
                profile,
                "--output",
                "json",
                "ec2",
                "describe-regions",
            ],
        )
        .await
        {
            Err(_) => return,
            Ok(regions) => {
                for region in regions["Regions"].as_array().unwrap() {
                    let region_name = region["RegionName"].as_str().unwrap_or("");
                    state
                        .options
                        .push((region_name.to_string(), region_name.to_string()));
                }
            }
        };
    }

    async fn load_eks_clusters(&self, state: &mut ImportViewState, profile: &str, region: &str) {
        match exec_to_json(
            "aws",
            &[
                "--profile",
                profile,
                "--output",
                "json",
                "eks",
                "list-clusters",
                "--region",
                region,
            ],
        )
        .await
        {
            Err(_) => return,
            Ok(clusters) => {
                for cluster in clusters["clusters"].as_array().unwrap() {
                    let cluster_name = cluster.as_str().unwrap_or("");
                    state
                        .options
                        .push((cluster_name.to_string(), cluster_name.to_string()));
                }
            }
        };
    }

    async fn import_aws_cluster(&self, import_path: &CloudImportPath) {
        match exec_to_str(
            "aws",
            &[
                "--region",
                import_path.get_aws_region().as_str(),
                "--profile",
                import_path.get_aws_profile().as_str(),
                "eks",
                "update-kubeconfig",
                "--name",
                import_path.get_cluster_id().as_str(),
            ],
        )
        .await
        {
            Err(_) => {}
            Ok(_) => {}
        };
    }

    async fn import_gcp_cluster(&self, import_path: &CloudImportPath) {
        match exec_to_str(
            "gcloud",
            &[
                "container",
                "clusters",
                "get-credentials",
                import_path.get_cluster_id().as_str(),
                "--project",
                import_path.get_gcp_project().as_str(),
            ],
        )
        .await
        {
            Err(_) => {}
            Ok(_) => {}
        };
    }

    async fn import_azure_cluster(&self, import_path: &CloudImportPath) {
        match exec_to_str(
            "az",
            &[
                "aks",
                "get-credentials",
                "--resource-group",
                import_path.get_azure_resource_group().as_str(),
                "--name",
                import_path.get_cluster_id().as_str(),
                "--subscription",
                import_path.get_azure_subscription().as_str(),
                "--overwrite-existing",
            ],
        )
        .await
        {
            Err(_) => {}
            Ok(_) => {}
        };
    }

    async fn import_cluster(&self, import_path: &CloudImportPath) {
        if import_path.is_aws() {
            self.import_aws_cluster(import_path).await;
        } else if import_path.is_gcp() {
            self.import_gcp_cluster(import_path).await;
        } else if import_path.is_azure() {
            self.import_azure_cluster(import_path).await;
        }
    }

    async fn load_aks_clusters(
        &self,
        state: &mut ImportViewState,
        subscription: &str,
        resource_group: &str,
    ) {
        match exec_to_json(
            "az",
            &[
                "aks",
                "list",
                "--subscription",
                subscription,
                "--resource-group",
                resource_group,
                "--output",
                "json",
            ],
        )
        .await
        {
            Err(_) => return,
            Ok(clusters) => {
                for cluster in clusters.as_array().unwrap() {
                    let cluster_name = cluster["name"].as_str().unwrap_or("");
                    let cluster_id = cluster["id"].as_str().unwrap_or("");
                    state
                        .options
                        .push((cluster_id.to_string(), cluster_name.to_string()));
                }
            }
        };
    }

    async fn load_azure_subscriptions(&self, state: &mut ImportViewState) {
        match exec_to_json("az", &["account", "list", "--output", "json"]).await {
            Err(_) => return,
            Ok(subscriptions) => {
                for subscription in subscriptions.as_array().unwrap() {
                    let subscription_id = subscription["id"].as_str().unwrap_or("");
                    let subscription_name = subscription["name"].as_str().unwrap_or("");
                    if !subscription_id.is_empty() && !subscription_name.is_empty() {
                        state
                            .options
                            .push((subscription_id.to_string(), subscription_name.to_string()));
                    }
                }
            }
        };
    }

    async fn load_azure_resource_groups(&self, state: &mut ImportViewState, subscription: &str) {
        match exec_to_json(
            "az",
            &[
                "group",
                "list",
                "--subscription",
                subscription,
                "--output",
                "json",
            ],
        )
        .await
        {
            Err(_) => return,
            Ok(resource_groups) => {
                for resource_group in resource_groups.as_array().unwrap() {
                    let resource_group_name = resource_group["name"].as_str().unwrap_or("");
                    state.options.push((
                        resource_group_name.to_string(),
                        resource_group_name.to_string(),
                    ));
                }
            }
        };
    }

    pub async fn load_options(&self) {
        let mut state_lock = self.state.lock().await;
        let state = ImportViewState::from_view_state(&mut state_lock);

        if self.import_path.is_terminal() {
            return;
        }
        if self.import_path.is_empty() {
            self.load_cloud_options(state).await;
        } else {
            if self.import_path.is_gcp() {
                if !self.import_path.has_gcp_project() {
                    self.load_gcp_projects(state).await;
                } else {
                    self.load_gke_clusters(state, self.import_path.get_gcp_project().as_str())
                        .await;
                }
            } else if self.import_path.is_aws() {
                if !self.import_path.has_aws_profile() {
                    self.load_aws_profiles(state).await;
                } else {
                    if !self.import_path.has_aws_region() {
                        self.load_aws_regions(state, self.import_path.get_aws_profile().as_str())
                            .await;
                    } else {
                        self.load_eks_clusters(
                            state,
                            self.import_path.get_aws_profile().as_str(),
                            self.import_path.get_aws_region().as_str(),
                        )
                        .await;
                    }
                }
            } else if self.import_path.is_azure() {
                if !self.import_path.has_azure_subscription() {
                    self.load_azure_subscriptions(state).await;
                } else {
                    if !self.import_path.has_azure_resource_group() {
                        self.load_azure_resource_groups(
                            state,
                            self.import_path.get_azure_subscription().as_str(),
                        )
                        .await;
                    } else {
                        self.load_aks_clusters(
                            state,
                            self.import_path.get_azure_subscription().as_str(),
                            self.import_path.get_azure_resource_group().as_str(),
                        )
                        .await;
                    }
                }
            }
        }
    }

    async fn handle_keyboard(
        &self,
        event: Event,
        _state: &AppState,
        view_state: &mut ImportViewState,
    ) -> Option<KtxEvent> {
        // let list_state = &view_state.list_state;
        if let Some(event) = handle_list_navigation_keyboard_event(
            event,
            self.event_bus_tx.clone(),
            &mut view_state.remembered_g,
        )
        .await
        {
            match event {
                Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                }) => {
                    let _ = self.event_bus_tx.send(KtxEvent::PopView).await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) => {
                    let selected_option = view_state
                        .options
                        .get(view_state.list_state.selected().unwrap())
                        .unwrap();
                    let import_path = self.import_path.push_clone(selected_option.clone());
                    if import_path.is_terminal() {
                        self.import_cluster(&import_path).await;
                        let _ = self.event_bus_tx.send(KtxEvent::RefreshConfig).await;
                    } else {
                        let _ = self
                            .event_bus_tx
                            .send(KtxEvent::ShowImportView(import_path))
                            .await;
                    }
                }
                _ => {
                    view_state.remembered_g = false;
                    return Some(KtxEvent::TerminalEvent(event));
                }
            }
        };
        None
    }

    async fn handle_app_event(
        &self,
        event: KtxEvent,
        _state: &AppState,
        view_state: &mut ImportViewState,
    ) -> Option<KtxEvent> {
        let list_state = &mut view_state.list_state;
        if let Some(event) =
            handle_list_navigation_event(event, list_state, view_state.options.len()).await
        {
            match event {
                // Handle non-navigation events here
                _ => Some(event),
            }
        } else {
            None
        }
    }
}

#[async_trait]
impl<B> AppView<B> for ImportView
where
    B: Backend + Sync + Send,
{
    fn get_state_mutex(&self) -> Arc<Mutex<ViewState>> {
        self.state.clone()
    }

    fn draw_top_bar(&self, _state: &AppState) -> Paragraph<'_> {
        Paragraph::new(Line::from(vec![
            key_style("jk"),
            action_style(" - up/down, "),
            key_style("Enter"),
            action_style(" - select, "),
        ]))
    }

    fn draw(&self, f: &mut Frame<B>, area: Rect, _state: &AppState, view_state: &mut ViewState) {
        let view_state = ImportViewState::from_view_state(view_state);
        let items: Vec<ListItem> = view_state
            .options
            .iter()
            .map(|opt| ListItem::new(opt.1.clone()))
            .collect();
        let list = styled_list("Configured Cloud Providers", items);
        f.render_stateful_widget(list, area, &mut view_state.list_state);
    }

    async fn handle_event(&self, event: KtxEvent, state: &AppState) -> Option<KtxEvent> {
        let mut locked_state = self.state.lock().await;
        let view_state = ImportViewState::from_view_state(&mut locked_state);
        match event {
            KtxEvent::TerminalEvent(evt) => self.handle_keyboard(evt, state, view_state).await,
            _ => self.handle_app_event(event, state, view_state).await,
        }
    }
}
