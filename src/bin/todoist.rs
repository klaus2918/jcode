use anyhow::{Context, Result, bail};
use clap::{Args as ClapArgs, Parser, Subcommand};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const DEFAULT_TOKEN_ENV: &str = "TODOIST_API_TOKEN";
const DEFAULT_BASE_URL: &str = "https://api.todoist.com/rest/v2";

#[derive(Parser, Debug)]
#[command(name = "todoist")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "A small Todoist CLI built in Rust")]
struct Cli {
    /// Environment variable that stores the Todoist API token
    #[arg(long, default_value = DEFAULT_TOKEN_ENV)]
    token_env: String,

    /// Todoist REST API base URL
    #[arg(long, default_value = DEFAULT_BASE_URL)]
    base_url: String,

    /// Emit JSON instead of human-readable output
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Project management commands
    #[command(subcommand)]
    Projects(ProjectCommand),

    /// Task management commands
    #[command(subcommand)]
    Tasks(TaskCommand),
}

#[derive(Subcommand, Debug)]
enum ProjectCommand {
    /// List projects
    #[command(alias = "ls")]
    List,
}

#[derive(Subcommand, Debug)]
enum TaskCommand {
    /// List tasks
    #[command(alias = "ls")]
    List(TaskListArgs),

    /// Show one task by ID
    Show {
        /// Todoist task ID
        task: String,
    },

    /// Create a task
    #[command(alias = "create")]
    Add(TaskAddArgs),

    /// Mark a task complete
    #[command(alias = "close")]
    Done {
        /// Todoist task ID
        task: String,
    },

    /// Delete a task
    #[command(alias = "rm")]
    Delete {
        /// Todoist task ID
        task: String,
    },
}

#[derive(ClapArgs, Debug)]
struct TaskListArgs {
    /// Todoist filter expression, e.g. "today & #Inbox"
    #[arg(long)]
    filter: Option<String>,

    /// Project name or ID to filter by
    #[arg(long)]
    project: Option<String>,

    /// Section ID to filter by
    #[arg(long)]
    section_id: Option<String>,
}

#[derive(ClapArgs, Debug)]
struct TaskAddArgs {
    /// Task content
    content: String,

    /// Task description
    #[arg(long)]
    description: Option<String>,

    /// Project name or ID
    #[arg(long)]
    project: Option<String>,

    /// Section ID
    #[arg(long)]
    section_id: Option<String>,

    /// Due date in natural language, e.g. "tomorrow at 9am"
    #[arg(long)]
    due: Option<String>,

    /// Due date in YYYY-MM-DD format
    #[arg(long)]
    due_date: Option<String>,

    /// Priority from 1 (low) to 4 (urgent)
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4))]
    priority: Option<u8>,

    /// Repeatable label name
    #[arg(long = "label")]
    labels: Vec<String>,
}

#[derive(Debug, Clone)]
struct TodoistClient {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl TodoistClient {
    fn new(base_url: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("todoist-cli/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("failed to construct HTTP client")?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
        })
    }

    async fn get<T>(&self, path: &str, query: &[(&str, String)]) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = self
            .http
            .get(self.url(path))
            .bearer_auth(&self.token)
            .query(query)
            .send()
            .await
            .with_context(|| format!("request failed: GET {path}"))?;
        decode_json(response).await
    }

    async fn post<B, T>(&self, path: &str, body: &B) -> Result<T>
    where
        B: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let response = self
            .http
            .post(self.url(path))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .with_context(|| format!("request failed: POST {path}"))?;
        decode_json(response).await
    }

    async fn post_empty(&self, path: &str) -> Result<()> {
        let response = self
            .http
            .post(self.url(path))
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("request failed: POST {path}"))?;
        ensure_success(response).await
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let response = self
            .http
            .delete(self.url(path))
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("request failed: DELETE {path}"))?;
        ensure_success(response).await
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Project {
    id: String,
    name: String,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    is_favorite: Option<bool>,
    #[serde(default)]
    view_style: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Due {
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    datetime: Option<String>,
    #[serde(default)]
    string: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    is_recurring: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Task {
    id: String,
    content: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    section_id: Option<String>,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    creator_id: Option<String>,
    #[serde(default)]
    assignee_id: Option<String>,
    #[serde(default)]
    assigner_id: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    priority: u8,
    #[serde(default)]
    comment_count: Option<u64>,
    #[serde(default)]
    is_completed: Option<bool>,
    #[serde(default)]
    due: Option<Due>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateTaskRequest {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    section_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    due_string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    due_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    labels: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let token = read_token(&cli.token_env)?;
    let client = TodoistClient::new(cli.base_url, token)?;

    match cli.command {
        Command::Projects(ProjectCommand::List) => {
            let mut projects: Vec<Project> = client.get("projects", &[]).await?;
            projects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            if cli.json {
                print_json(&projects)?;
            } else {
                print_projects(&projects);
            }
        }
        Command::Tasks(TaskCommand::List(args)) => {
            let project_id = match args.project.as_deref() {
                Some(project_ref) => Some(resolve_project_ref(&client, project_ref).await?.id),
                None => None,
            };
            let query = build_task_list_query(&args, project_id);
            let tasks: Vec<Task> = client.get("tasks", &query).await?;
            if cli.json {
                print_json(&tasks)?;
            } else {
                let projects = list_projects_by_id(&client).await?;
                print_task_list(&tasks, &projects);
            }
        }
        Command::Tasks(TaskCommand::Show { task }) => {
            let task: Task = client.get(&format!("tasks/{task}"), &[]).await?;
            if cli.json {
                print_json(&task)?;
            } else {
                let projects = list_projects_by_id(&client).await?;
                print_task_details(&task, &projects);
            }
        }
        Command::Tasks(TaskCommand::Add(args)) => {
            if args.due.is_some() && args.due_date.is_some() {
                bail!("--due and --due-date are mutually exclusive")
            }
            let project_id = match args.project.as_deref() {
                Some(project_ref) => Some(resolve_project_ref(&client, project_ref).await?.id),
                None => None,
            };
            let request = CreateTaskRequest {
                content: args.content,
                description: args.description,
                project_id,
                section_id: args.section_id,
                due_string: args.due,
                due_date: args.due_date,
                priority: args.priority,
                labels: args.labels,
            };
            let task: Task = client.post("tasks", &request).await?;
            if cli.json {
                print_json(&task)?;
            } else {
                println!("Created task {}: {}", task.id, task.content);
                if let Some(url) = task.url.as_deref() {
                    println!("URL: {url}");
                }
            }
        }
        Command::Tasks(TaskCommand::Done { task }) => {
            client.post_empty(&format!("tasks/{task}/close")).await?;
            if cli.json {
                print_json(&serde_json::json!({"status": "ok", "task": task, "action": "done"}))?;
            } else {
                println!("Completed task {task}");
            }
        }
        Command::Tasks(TaskCommand::Delete { task }) => {
            client.delete(&format!("tasks/{task}")).await?;
            if cli.json {
                print_json(&serde_json::json!({"status": "ok", "task": task, "action": "delete"}))?;
            } else {
                println!("Deleted task {task}");
            }
        }
    }

    Ok(())
}

fn read_token(token_env: &str) -> Result<String> {
    match std::env::var(token_env) {
        Ok(token) if !token.trim().is_empty() => Ok(token),
        _ => bail!(
            "missing Todoist API token: set the {} environment variable before running this command",
            token_env
        ),
    }
}

fn build_task_list_query(
    args: &TaskListArgs,
    project_id: Option<String>,
) -> Vec<(&'static str, String)> {
    let mut query = Vec::new();
    if let Some(filter) = args.filter.as_ref() {
        query.push(("filter", filter.clone()));
    }
    if let Some(project_id) = project_id {
        query.push(("project_id", project_id));
    }
    if let Some(section_id) = args.section_id.as_ref() {
        query.push(("section_id", section_id.clone()));
    }
    query
}

async fn resolve_project_ref(client: &TodoistClient, project_ref: &str) -> Result<Project> {
    let projects: Vec<Project> = client.get("projects", &[]).await?;
    find_project(&projects, project_ref)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("project not found: {project_ref}"))
}

async fn list_projects_by_id(
    client: &TodoistClient,
) -> Result<std::collections::HashMap<String, Project>> {
    let projects: Vec<Project> = client.get("projects", &[]).await?;
    Ok(projects
        .into_iter()
        .map(|project| (project.id.clone(), project))
        .collect())
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).context("failed to serialize JSON output")?
    );
    Ok(())
}

fn find_project<'a>(projects: &'a [Project], project_ref: &str) -> Option<&'a Project> {
    projects
        .iter()
        .find(|project| project.id == project_ref)
        .or_else(|| projects.iter().find(|project| project.name == project_ref))
        .or_else(|| {
            projects
                .iter()
                .find(|project| project.name.eq_ignore_ascii_case(project_ref))
        })
}

fn print_projects(projects: &[Project]) {
    if projects.is_empty() {
        println!("No projects.");
        return;
    }

    for project in projects {
        println!("{}\t{}", project.id, project.name);
    }
}

fn print_task_list(tasks: &[Task], projects: &std::collections::HashMap<String, Project>) {
    if tasks.is_empty() {
        println!("No tasks.");
        return;
    }

    for task in tasks {
        let project_name = task
            .project_id
            .as_ref()
            .and_then(|id| projects.get(id))
            .map(|project| project.name.as_str())
            .unwrap_or("unknown-project");
        let due = format_due(task.due.as_ref());
        let labels = if task.labels.is_empty() {
            String::new()
        } else {
            format!(" labels:{}", task.labels.join(","))
        };
        println!(
            "{}\t[P{}] {} ({}){}{}",
            task.id,
            task.priority.max(1),
            task.content,
            project_name,
            due,
            labels
        );
    }
}

fn print_task_details(task: &Task, projects: &std::collections::HashMap<String, Project>) {
    println!("ID: {}", task.id);
    println!("Content: {}", task.content);
    if !task.description.trim().is_empty() {
        println!("Description: {}", task.description);
    }
    if let Some(project_name) = task
        .project_id
        .as_ref()
        .and_then(|id| projects.get(id))
        .map(|project| project.name.as_str())
    {
        println!("Project: {}", project_name);
    }
    println!("Priority: {}", task.priority.max(1));
    if let Some(due) = task.due.as_ref() {
        if let Some(date) = due.date.as_deref() {
            println!("Due date: {}", date);
        }
        if let Some(datetime) = due.datetime.as_deref() {
            println!("Due datetime: {}", datetime);
        }
        if let Some(string) = due.string.as_deref() {
            println!("Due string: {}", string);
        }
    }
    if !task.labels.is_empty() {
        println!("Labels: {}", task.labels.join(", "));
    }
    if let Some(url) = task.url.as_deref() {
        println!("URL: {}", url);
    }
}

fn format_due(due: Option<&Due>) -> String {
    match due {
        Some(due) => {
            if let Some(datetime) = due.datetime.as_deref() {
                format!(" due:{}", datetime)
            } else if let Some(date) = due.date.as_deref() {
                format!(" due:{}", date)
            } else if let Some(string) = due.string.as_deref() {
                format!(" due:{}", string)
            } else {
                String::new()
            }
        }
        None => String::new(),
    }
}

async fn decode_json<T>(response: reqwest::Response) -> Result<T>
where
    T: DeserializeOwned,
{
    let response = error_for_status(response).await?;
    response
        .json::<T>()
        .await
        .context("failed to decode Todoist API response")
}

async fn ensure_success(response: reqwest::Response) -> Result<()> {
    error_for_status(response).await?;
    Ok(())
}

async fn error_for_status(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response.text().await.unwrap_or_default();
    let detail = if body.trim().is_empty() {
        status
            .canonical_reason()
            .unwrap_or("request failed")
            .to_string()
    } else {
        body
    };

    match status {
        StatusCode::UNAUTHORIZED => bail!("Todoist API unauthorized: {}", detail.trim()),
        StatusCode::FORBIDDEN => bail!("Todoist API forbidden: {}", detail.trim()),
        StatusCode::NOT_FOUND => bail!("Todoist resource not found: {}", detail.trim()),
        _ => bail!("Todoist API error ({}): {}", status.as_u16(), detail.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_task_list_query_includes_selected_filters() {
        let args = TaskListArgs {
            filter: Some("today".to_string()),
            project: Some("Inbox".to_string()),
            section_id: Some("sec-1".to_string()),
        };
        let query = build_task_list_query(&args, Some("proj-1".to_string()));
        assert_eq!(
            query,
            vec![
                ("filter", "today".to_string()),
                ("project_id", "proj-1".to_string()),
                ("section_id", "sec-1".to_string()),
            ]
        );
    }

    #[test]
    fn format_due_prefers_datetime_then_date_then_string() {
        let datetime_due = Due {
            date: Some("2026-04-07".to_string()),
            datetime: Some("2026-04-07T09:00:00Z".to_string()),
            string: Some("today".to_string()),
            timezone: None,
            is_recurring: None,
        };
        assert_eq!(format_due(Some(&datetime_due)), " due:2026-04-07T09:00:00Z");

        let date_due = Due {
            date: Some("2026-04-07".to_string()),
            datetime: None,
            string: Some("today".to_string()),
            timezone: None,
            is_recurring: None,
        };
        assert_eq!(format_due(Some(&date_due)), " due:2026-04-07");
    }

    #[test]
    fn read_token_rejects_missing_env() {
        let name = "TODOIST_TEST_TOKEN_SHOULD_NOT_EXIST";
        unsafe { std::env::remove_var(name) };
        let error = read_token(name).unwrap_err().to_string();
        assert!(error.contains(name));
    }

    #[test]
    fn resolve_project_ref_matches_id_name_and_case_insensitive_name() {
        let projects = vec![
            Project {
                id: "123".to_string(),
                name: "Inbox".to_string(),
                color: None,
                is_favorite: None,
                view_style: None,
                url: None,
            },
            Project {
                id: "456".to_string(),
                name: "Work".to_string(),
                color: None,
                is_favorite: None,
                view_style: None,
                url: None,
            },
        ];

        assert_eq!(find_project(&projects, "123").unwrap().name, "Inbox");
        assert_eq!(find_project(&projects, "Work").unwrap().id, "456");
        assert_eq!(find_project(&projects, "inbox").unwrap().id, "123");
        assert!(find_project(&projects, "missing").is_none());
    }
}
