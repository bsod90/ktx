use std::{error::Error, sync::Arc, time::Duration};

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
    app::{AppState, HandleEventResult},
    types::{CloudImportPath, EmptyResult, KtxEvent, ViewState},
    AppView,
};

use super::utils::{
    action_style, handle_list_navigation_event, handle_list_navigation_keyboard_event, key_style,
    styled_list,
};

type ImportOption = (String, String, Option<String>);

pub struct ImportViewState {
    pub list_state: ListState,
    pub remembered_g: bool,
    pub options: Vec<ImportOption>,
    pub filter: String,
}

impl ImportViewState {
    fn get_filtered_options(&self) -> Vec<ImportOption> {
        let mut filtered_options = self.options.clone();
        filtered_options
            .retain(|(_, name, _)| name.to_lowercase().contains(&self.filter.to_lowercase()));
        filtered_options
    }

    fn get_selected_option(&self) -> ImportOption {
        let filtered_options = self.get_filtered_options();
        let selected_index = self.list_state.selected().unwrap();
        filtered_options.get(selected_index).unwrap().clone()
    }
}

pub struct ImportView {
    event_bus_tx: mpsc::Sender<KtxEvent>,
    state: Arc<Mutex<ViewState>>,
    import_path: CloudImportPath,
}

async fn exec_to_str(cmd: &str, args: &[&str]) -> Result<String, Box<dyn Error + Send + Sync>> {
    let output = tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await?;
    if output.status.success() == false {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            stderr.to_string(),
        )));
    }
    let output = String::from_utf8_lossy(&output.stdout);
    Ok(output.to_string())
}

async fn exec_to_json(
    cmd: &str,
    args: &[&str],
) -> Result<serde_json::Value, Box<dyn Error + Send + Sync>> {
    let output = exec_to_str(cmd, args).await?;
    let json: serde_json::Value = serde_json::from_str(&output)?;
    Ok(json)
}

async fn import_aws_cluster(import_path: &CloudImportPath) -> EmptyResult {
    exec_to_str(
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
    .await?;
    Ok(())
}

async fn import_gke_cluster(import_path: &CloudImportPath) -> EmptyResult {
    exec_to_str(
        "gcloud",
        &[
            "container",
            "clusters",
            "get-credentials",
            import_path.get_cluster_id().as_str(),
            "--zone",
            import_path.get_gke_zone().as_str(),
            "--project",
            import_path.get_gcp_project().as_str(),
        ],
    )
    .await?;
    Ok(())
}

async fn import_aks_cluster(import_path: &CloudImportPath) -> EmptyResult {
    exec_to_str(
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
    .await?;
    Ok(())
}

async fn import_cluster(
    import_path: &CloudImportPath,
    event_bus_tx: mpsc::Sender<KtxEvent>,
    config_lock: Arc<Mutex<()>>,
) -> EmptyResult {
    let _config_guard = config_lock.lock().await;
    if import_path.is_aws() {
        import_aws_cluster(import_path).await?;
    } else if import_path.is_gcp() {
        import_gke_cluster(import_path).await?;
    } else if import_path.is_azure() {
        import_aks_cluster(import_path).await?;
    }
    let _ = event_bus_tx
        .send(KtxEvent::PushSuccessMessage(format!(
            "Successfully imported {}",
            import_path.get_cluster_id()
        )))
        .await;
    // This is to ensure all buffers have been flushed and there're no conflicts between
    // simultaneous import operations.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    Ok(())
}

impl ImportView {
    pub fn new<B: Backend>(
        event_bus_tx: mpsc::Sender<KtxEvent>,
        import_path: CloudImportPath,
    ) -> Self {
        let state = ImportViewState {
            list_state: ListState::default(),
            remembered_g: false,
            options: vec![],
            filter: "".to_string(),
        };
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

    async fn load_cloud_options(&self, state: &mut ImportViewState) -> EmptyResult {
        let (gcp_configured, aws_configured, azure_configured) = tokio::join!(
            self.is_gcp_configured(),
            self.is_aws_configured(),
            self.is_azure_configured()
        );
        if aws_configured {
            state
                .options
                .push(("aws".to_string(), "AWS".to_string(), None));
        }
        if gcp_configured {
            state
                .options
                .push(("gcp".to_string(), "GCP".to_string(), None));
        }
        if azure_configured {
            state
                .options
                .push(("azure".to_string(), "Azure".to_string(), None));
        };
        Ok(())
    }

    async fn load_gcp_projects(&self, state: &mut ImportViewState) -> EmptyResult {
        let projects = exec_to_json("gcloud", &["--format", "json", "projects", "list"]).await?;
        for project in projects.as_array().unwrap() {
            let project_id = project["projectId"].as_str().unwrap_or("");
            let project_name = project["name"].as_str().unwrap_or("");
            let lifecycle_state = project["lifecycleState"].as_str().unwrap_or("");
            if !project_id.is_empty()
                && !project_id.starts_with("sys-")
                && !project_name.is_empty()
                && lifecycle_state == "ACTIVE"
            {
                state.options.push((
                    project_id.to_string(),
                    format!("{} ({})", project_name.to_string(), project_id.to_string()),
                    None,
                ));
            }
        }
        Ok(())
    }

    async fn load_gke_clusters(&self, state: &mut ImportViewState, project: &str) -> EmptyResult {
        let clusters = exec_to_json(
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
        .await?;
        for cluster in clusters.as_array().unwrap() {
            let cluster_name = cluster["name"].as_str().unwrap_or("");
            let zone = cluster["zone"].as_str().unwrap_or("");
            state.options.push((
                cluster_name.to_string(),
                cluster_name.to_string(),
                Some(zone.to_string()),
            ));
        }
        Ok(())
    }

    async fn load_aws_profiles(&self, state: &mut ImportViewState) -> EmptyResult {
        let output = exec_to_str("aws", &["configure", "list-profiles"]).await?;
        let profiles = output.split("\n").collect::<Vec<&str>>();
        for profile in profiles {
            if !profile.is_empty() {
                state
                    .options
                    .push((profile.to_string(), profile.to_string(), None));
            }
        }
        Ok(())
    }

    async fn load_aws_regions(&self, state: &mut ImportViewState, profile: &str) -> EmptyResult {
        let regions = exec_to_json(
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
        .await?;
        for region in regions["Regions"].as_array().unwrap() {
            let region_name = region["RegionName"].as_str().unwrap_or("");
            state
                .options
                .push((region_name.to_string(), region_name.to_string(), None));
        }
        Ok(())
    }

    async fn load_eks_clusters(
        &self,
        state: &mut ImportViewState,
        profile: &str,
        region: &str,
    ) -> EmptyResult {
        let clusters = exec_to_json(
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
        .await?;
        for cluster in clusters["clusters"].as_array().unwrap() {
            let cluster_name = cluster.as_str().unwrap_or("");
            state
                .options
                .push((cluster_name.to_string(), cluster_name.to_string(), None));
        }
        Ok(())
    }

    async fn load_aks_clusters(
        &self,
        state: &mut ImportViewState,
        subscription: &str,
    ) -> EmptyResult {
        let clusters = exec_to_json(
            "az",
            &[
                "aks",
                "list",
                "--subscription",
                subscription,
                "--output",
                "json",
            ],
        )
        .await?;
        for cluster in clusters.as_array().unwrap() {
            let cluster_name = cluster["name"].as_str().unwrap_or("");
            let resource_group = cluster["resourceGroup"].as_str().unwrap_or("");
            state.options.push((
                cluster_name.to_string(),
                format!(
                    "{} (RG: {})",
                    cluster_name.to_string(),
                    resource_group.to_string()
                ),
                Some(resource_group.to_string()),
            ));
        }
        Ok(())
    }

    async fn load_azure_subscriptions(&self, state: &mut ImportViewState) -> EmptyResult {
        let subscriptions = exec_to_json("az", &["account", "list", "--output", "json"]).await?;
        for subscription in subscriptions.as_array().unwrap() {
            let subscription_id = subscription["id"].as_str().unwrap_or("");
            let subscription_name = subscription["name"].as_str().unwrap_or("");
            if !subscription_id.is_empty() && !subscription_name.is_empty() {
                state.options.push((
                    subscription_id.to_string(),
                    format!(
                        "{} ({})",
                        subscription_name.to_string(),
                        subscription_id.to_string()
                    ),
                    None,
                ));
            }
        }
        Ok(())
    }

    async fn drilldown_import_path(&self, state: &mut ImportViewState) -> EmptyResult {
        match (
            self.import_path.get_platform().as_str(),
            self.import_path.len(),
        ) {
            ("aws", 1) => {
                self.load_aws_profiles(state).await?;
            }
            ("aws", 2) => {
                self.load_aws_regions(state, self.import_path.get_aws_profile().as_str())
                    .await?;
            }
            ("aws", 3) => {
                self.load_eks_clusters(
                    state,
                    self.import_path.get_aws_profile().as_str(),
                    self.import_path.get_aws_region().as_str(),
                )
                .await?;
            }
            ("gcp", 1) => {
                self.load_gcp_projects(state).await?;
            }
            ("gcp", 2) => {
                self.load_gke_clusters(state, self.import_path.get_gcp_project().as_str())
                    .await?;
            }
            ("azure", 1) => {
                self.load_azure_subscriptions(state).await?;
            }
            ("azure", 2) => {
                self.load_aks_clusters(state, self.import_path.get_azure_subscription().as_str())
                    .await?;
            }
            _ => {}
        };
        Ok(())
    }

    pub async fn load_options(&self) -> EmptyResult {
        let mut state_lock = self.state.lock().await;
        let state = ImportViewState::from_view_state(&mut state_lock);
        if self.import_path.is_full() {
            return Ok(());
        }
        if self.import_path.is_empty() {
            self.load_cloud_options(state).await?;
        } else {
            self.drilldown_import_path(state).await?;
        }
        if !state.options.is_empty() {
            state.list_state.select(Some(0));
        };
        Ok(())
    }

    async fn handle_enter(
        &self,
        view_state: &mut ImportViewState,
        config_lock: Arc<Mutex<()>>,
    ) -> EmptyResult {
        if !view_state.get_filtered_options().is_empty()
            && view_state.list_state.selected().is_some()
        {
            let selected_option = view_state.get_selected_option();
            let import_path = self.import_path.push_clone(selected_option.clone());
            if import_path.is_full() {
                import_cluster(&import_path, self.event_bus_tx.clone(), config_lock.clone())
                    .await?;
                let _ = self.event_bus_tx.send(KtxEvent::RefreshConfig).await;
            } else {
                let _ = self
                    .event_bus_tx
                    .send(KtxEvent::ShowImportView(import_path))
                    .await;
            }
        }
        Ok(())
    }

    async fn import_all(
        &self,
        view_state: &mut ImportViewState,
        config_lock: Arc<Mutex<()>>,
    ) -> EmptyResult {
        let selected_options = view_state.get_filtered_options();
        let import_path = self.import_path.clone();
        let event_bus = self.event_bus_tx.clone();
        tokio::spawn(async move {
            for option in selected_options {
                let import_path = import_path.push_clone(option.clone());
                if let Err(e) =
                    import_cluster(&import_path, event_bus.clone(), config_lock.clone()).await
                {
                    let _ = event_bus
                        .send(KtxEvent::PushErrorMessage(e.to_string()))
                        .await;
                } else {
                    let _ = event_bus.send(KtxEvent::RefreshConfig).await;
                };
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        Ok(())
    }

    async fn handle_keyboard(
        &self,
        event: Event,
        state: &AppState,
        view_state: &mut ImportViewState,
    ) -> HandleEventResult {
        if let Some(event) = handle_list_navigation_keyboard_event(
            event,
            self.event_bus_tx.clone(),
            &mut view_state.remembered_g,
        )
        .await?
        {
            match event {
                Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                }) => {
                    let _ = self.event_bus_tx.send(KtxEvent::PopView).await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char('a'),
                    ..
                }) => {
                    if self.import_path.is_listing_clusters() {
                        self.import_all(view_state, state.config_lock.clone())
                            .await?;
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) => {
                    self.handle_enter(view_state, state.config_lock.clone())
                        .await?;
                }
                _ => {
                    view_state.remembered_g = false;
                    return Ok(Some(KtxEvent::TerminalEvent(event)));
                }
            }
        };
        Ok(None)
    }

    async fn handle_app_event(
        &self,
        event: KtxEvent,
        _state: &AppState,
        view_state: &mut ImportViewState,
    ) -> HandleEventResult {
        let options_len = view_state.get_filtered_options().len();
        let list_state = &mut view_state.list_state;
        if let Some(event) = handle_list_navigation_event(event, list_state, options_len).await? {
            match event {
                // Handle non-navigation events here
                _ => Ok(Some(event)),
            }
        } else {
            Ok(None)
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

    async fn update_filter(&self, filter: String) {
        let mut state = self.state.lock().await;
        let mut state = ImportViewState::from_view_state(&mut state);
        state.filter = filter;
    }

    async fn get_filter(&self) -> String {
        let mut state = self.state.lock().await;
        let state = ImportViewState::from_view_state(&mut state);
        state.filter.clone()
    }

    fn draw_top_bar(&self, _state: &AppState) -> Paragraph<'_> {
        if self.import_path.is_listing_clusters() {
            Paragraph::new(Line::from(vec![
                key_style("jk"),
                action_style(" - up/down, "),
                key_style("Enter"),
                action_style(" - import, "),
                key_style("a"),
                action_style(" - import all, "),
            ]))
        } else {
            Paragraph::new(Line::from(vec![
                key_style("jk"),
                action_style(" - up/down, "),
                key_style("Enter"),
                action_style(" - list, "),
            ]))
        }
    }

    fn draw(&self, f: &mut Frame<B>, area: Rect, _state: &AppState, view_state: &mut ViewState) {
        let view_state = ImportViewState::from_view_state(view_state);
        let items: Vec<ListItem> = view_state
            .get_filtered_options()
            .iter()
            .map(|opt| ListItem::new(opt.1.clone()))
            .collect();
        let list = styled_list("Import Kubernetes Context(s)", items);
        f.render_stateful_widget(list, area, &mut view_state.list_state);
    }

    async fn handle_event(&self, event: KtxEvent, state: &AppState) -> HandleEventResult {
        let mut locked_state = self.state.lock().await;
        let view_state = ImportViewState::from_view_state(&mut locked_state);
        match event {
            KtxEvent::TerminalEvent(evt) => self.handle_keyboard(evt, state, view_state).await,
            _ => self.handle_app_event(event, state, view_state).await,
        }
    }
}
