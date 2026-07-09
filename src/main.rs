mod api;
mod auth;
mod commands;
mod token_store;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "rustedin",
    version,
    about = "Multi-account LinkedIn CLI — post, share and reshare from personal accounts and company pages."
)]
struct Cli {
    /// Path to rustedin.json config file (default: next to the binary)
    #[arg(long, global = true)]
    config: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Configure LinkedIn app credentials
    Setup {
        /// App type: "personal" or "organization"
        #[arg(long)]
        app: String,
        /// LinkedIn app Client ID
        #[arg(long)]
        client_id: String,
        /// LinkedIn app Client Secret
        #[arg(long)]
        client_secret: String,
    },
    /// Authenticate a LinkedIn account via OAuth
    Auth {
        /// Account alias (e.g. "quentin", "entreprise")
        #[arg(long)]
        account: String,
        /// LinkedIn organization ID (only for company pages)
        #[arg(long)]
        org_id: Option<String>,
    },
    /// List all configured accounts
    Accounts,
    /// Show token expiry status for all accounts
    Status,
    /// Publish a text post
    Post {
        /// Account alias to post as
        #[arg(long)]
        account: String,
        /// Post content (max 3000 chars)
        #[arg(long)]
        text: String,
        /// Audience: PUBLIC (default), CONNECTIONS, or LOGGED_IN
        #[arg(long, default_value = "PUBLIC")]
        visibility: String,
    },
    /// Reshare an existing post from one or multiple accounts
    Reshare {
        /// Post URN to reshare (e.g. "urn:li:share:7123456789")
        #[arg(long)]
        post_id: String,
        /// Comma-separated account aliases, or "*" for all person accounts
        #[arg(long, value_delimiter = ',')]
        accounts: Vec<String>,
        /// Optional comment above the reshare
        #[arg(long)]
        commentary: Option<String>,
        /// Audience: PUBLIC (default), CONNECTIONS, or LOGGED_IN
        #[arg(long, default_value = "PUBLIC")]
        visibility: String,
    },
    /// Share an external article/URL
    Share {
        /// Account alias to post as
        #[arg(long)]
        account: String,
        /// URL of the article
        #[arg(long)]
        url: String,
        /// Article title
        #[arg(long)]
        title: String,
        /// Article description (used as commentary if --commentary is not set; LinkedIn ignores it in link previews)
        #[arg(long)]
        description: Option<String>,
        /// Personal comment above the article
        #[arg(long)]
        commentary: Option<String>,
        /// Audience: PUBLIC (default), CONNECTIONS, or LOGGED_IN
        #[arg(long, default_value = "PUBLIC")]
        visibility: String,
        /// Image: local file path or URL (used as article thumbnail, max 10 MB)
        #[arg(long)]
        image: Option<String>,
        /// Share mode: "article" (link preview card, default) or "image" (full image + link in text)
        #[arg(long, default_value = "article")]
        mode: String,
    },
    /// Retrieve a post by its URN (debug: check what LinkedIn stored)
    GetPost {
        /// Account alias (for authentication)
        #[arg(long)]
        account: String,
        /// Post URN (e.g. "urn:li:share:123456")
        #[arg(long)]
        post_id: String,
    },
    /// Get comments on a LinkedIn post
    Comments {
        /// Account alias (for authentication)
        #[arg(long)]
        account: String,
        /// Post URN (e.g. "urn:li:share:123456" or "urn:li:activity:123456")
        #[arg(long)]
        post_id: String,
        /// Number of comments to fetch (default: 20, max: 100)
        #[arg(long, default_value = "20")]
        count: u32,
        /// Starting index for pagination (default: 0)
        #[arg(long, default_value = "0")]
        start: u32,
    },
    /// Get profile info of a person
    Profile {
        /// Account alias (for authentication)
        #[arg(long)]
        account: String,
        /// URN of another person to look up (omit for your own profile)
        #[arg(long)]
        urn: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    token_store::init_path(cli.config.as_deref());

    let result = match cli.command {
        Commands::Setup {
            app,
            client_id,
            client_secret,
        } => commands::cmd_setup(&app, &client_id, &client_secret),
        Commands::Auth { account, org_id } => auth::run_auth(&account, org_id.as_deref()).await,
        Commands::Accounts => commands::cmd_accounts(),
        Commands::Status => commands::cmd_status(),
        Commands::Post {
            account,
            text,
            visibility,
        } => commands::cmd_post(&account, &text, &visibility.to_uppercase()).await,
        Commands::Reshare {
            post_id,
            accounts,
            commentary,
            visibility,
        } => {
            commands::cmd_reshare(
                &post_id,
                &accounts,
                commentary.as_deref(),
                &visibility.to_uppercase(),
            )
            .await
        }
        Commands::Share {
            account,
            url,
            title,
            description,
            commentary,
            visibility,
            image,
            mode,
        } => {
            commands::cmd_share(commands::ShareArgs {
                account: &account,
                url: &url,
                title: &title,
                description: description.as_deref(),
                commentary: commentary.as_deref(),
                visibility: &visibility.to_uppercase(),
                image: image.as_deref(),
                mode: &mode.to_lowercase(),
            })
            .await
        }
        Commands::GetPost { account, post_id } => commands::cmd_get_post(&account, &post_id).await,
        Commands::Comments {
            account,
            post_id,
            count,
            start,
        } => commands::cmd_comments(&account, &post_id, count, start).await,
        Commands::Profile { account, urn } => commands::cmd_profile(&account, urn.as_deref()).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
