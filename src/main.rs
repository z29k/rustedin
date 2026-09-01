//! rustedin — one binary, several social platforms.
//!
//! The CLI is grouped by platform (`rustedin linkedin post`,
//! `rustedin instagram reel`), with the cross-platform commands at the root:
//! `accounts`, `status`, `migrate` and `broadcast`.
//!
//! Output contract: **one JSON document on stdout**, everything else on stderr.

// A build with a platform compiled out legitimately leaves shared helpers
// unused: `MediaKind` without Meta, `urlencode` without LinkedIn, and so on.
// Full builds — the default, and what CI lints — stay strict.
#![cfg_attr(
    not(all(feature = "linkedin", feature = "meta")),
    allow(dead_code, unused_imports)
)]

#[cfg(not(any(feature = "linkedin", feature = "meta")))]
compile_error!(
    "rustedin needs at least one platform feature: `linkedin`, `meta`, or both (the default)."
);

mod broadcast;
mod commands;
mod core;
mod providers;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "rustedin",
    version,
    about = "Multi-account social CLI — publish to LinkedIn, Facebook Pages and Instagram.",
    after_help = "Every command prints a single JSON document on stdout; progress and warnings \
                  go to stderr.\n\nPlatform groups accept short aliases: li, fb, ig."
)]
struct Cli {
    /// Path to the rustedin.json config file (default: next to the binary)
    #[arg(long, global = true)]
    config: Option<String>,

    /// Graph API version to call, e.g. "v25.0" (Meta only; default: the version
    /// rustedin ships with)
    #[arg(long, global = true)]
    api_version: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List every configured account, grouped by platform
    Accounts,

    /// Show token status for every account, grouped by platform
    Status {
        /// Also ask Meta to validate each of its tokens (network call)
        #[arg(long)]
        check: bool,
    },

    /// Fold another config file into this one (e.g. a 1.x rustedin.json or a
    /// rustameta.json)
    Migrate {
        /// Path to the config file to import
        #[arg(long)]
        from: String,
        /// Overwrite accounts and credentials that already exist here
        #[arg(long)]
        force: bool,
    },

    /// Publish the same content to several platforms at once
    Broadcast(BroadcastArgs),

    /// LinkedIn personal accounts and company pages
    #[cfg(feature = "linkedin")]
    #[command(subcommand, alias = "li")]
    Linkedin(LinkedinCommands),

    /// Meta app, accounts and Pages (shared by Facebook and Instagram)
    #[cfg(feature = "meta")]
    #[command(subcommand)]
    Meta(MetaCommands),

    /// Publish to a Facebook Page
    #[cfg(feature = "meta")]
    #[command(subcommand, alias = "fb")]
    Facebook(FacebookCommands),

    /// Publish to an Instagram Professional account
    #[cfg(feature = "meta")]
    #[command(subcommand, alias = "ig")]
    Instagram(InstagramCommands),
}

// ---------------------------------------------------------------------------
// Cross-platform
// ---------------------------------------------------------------------------

#[derive(Args)]
struct BroadcastArgs {
    /// Comma-separated targets, each `platform:account[/page]`
    /// (e.g. `linkedin:quentin,facebook:z29k,instagram:z29k`)
    #[arg(long = "to", value_delimiter = ',', required = true)]
    targets: Vec<String>,

    /// The text: LinkedIn commentary, Facebook message, Instagram caption
    #[arg(long, default_value = "")]
    text: String,

    /// Local path or public URL. Required for an Instagram target
    #[arg(long)]
    image: Option<String>,

    /// URL to attach. LinkedIn and Facebook render a preview card; Instagram
    /// gets it appended to the caption
    #[arg(long)]
    link: Option<String>,

    /// Title for the platforms that need one (default: the first line of --text)
    #[arg(long)]
    title: Option<String>,

    /// LinkedIn audience: PUBLIC (default), CONNECTIONS or LOGGED_IN
    #[arg(long, default_value = "PUBLIC")]
    visibility: String,

    /// Facebook only: Unix timestamp or ISO 8601 date (10 min to 75 days ahead)
    #[arg(long)]
    schedule: Option<String>,

    /// Instagram only: alt text for accessibility
    #[arg(long)]
    alt_text: Option<String>,

    /// Delete the temporary Facebook photos used to relay local images
    #[arg(long)]
    cleanup_relay: bool,

    /// Print what each target would publish, without publishing anything
    #[arg(long)]
    dry_run: bool,
}

// ---------------------------------------------------------------------------
// LinkedIn
// ---------------------------------------------------------------------------

#[cfg(feature = "linkedin")]
#[derive(Subcommand)]
enum LinkedinCommands {
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
        /// Account alias (e.g. "quentin", "z29k")
        #[arg(long)]
        account: String,
        /// LinkedIn organization ID (only for company pages)
        #[arg(long)]
        org_id: Option<String>,
        /// Port for the local OAuth callback server
        #[arg(long, default_value_t = crate::core::oauth::DEFAULT_PORT)]
        port: u16,
    },

    /// List the configured LinkedIn accounts
    Accounts,

    /// Show LinkedIn token status
    Status,

    /// Publish a text post
    Post {
        /// Account alias to post as
        #[arg(long)]
        account: String,
        /// Post content (max 3000 characters)
        #[arg(long)]
        text: String,
        /// Audience: PUBLIC (default), CONNECTIONS or LOGGED_IN
        #[arg(long, default_value = "PUBLIC")]
        visibility: String,
    },

    /// Reshare an existing post from one or several accounts
    Reshare {
        /// Post URN to reshare (e.g. "urn:li:share:7123456789")
        #[arg(long)]
        post_id: String,
        /// Comma-separated account aliases, or "*" for every personal account
        #[arg(long, value_delimiter = ',')]
        accounts: Vec<String>,
        /// Optional comment above the reshare
        #[arg(long)]
        commentary: Option<String>,
        /// Audience: PUBLIC (default), CONNECTIONS or LOGGED_IN
        #[arg(long, default_value = "PUBLIC")]
        visibility: String,
    },

    /// Share an external article or URL
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
        /// Article description (used as commentary when --commentary is unset;
        /// LinkedIn ignores it in link previews)
        #[arg(long)]
        description: Option<String>,
        /// Personal comment above the article
        #[arg(long)]
        commentary: Option<String>,
        /// Audience: PUBLIC (default), CONNECTIONS or LOGGED_IN
        #[arg(long, default_value = "PUBLIC")]
        visibility: String,
        /// Image: local path or URL (article thumbnail, max 10 MB)
        #[arg(long)]
        image: Option<String>,
        /// "article" (link preview card, default) or "image" (full image + link
        /// in the text)
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

    /// Get the comments on a post
    Comments {
        /// Account alias (for authentication)
        #[arg(long)]
        account: String,
        /// Post URN (urn:li:share:xxx or urn:li:activity:xxx)
        #[arg(long)]
        post_id: String,
        /// Number of comments to fetch (max 100)
        #[arg(long, default_value = "20")]
        count: u32,
        /// Starting index for pagination
        #[arg(long, default_value = "0")]
        start: u32,
    },

    /// Get the profile of a member
    Profile {
        /// Account alias (for authentication)
        #[arg(long)]
        account: String,
        /// URN of another member to look up (omit for your own profile)
        #[arg(long)]
        urn: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Meta
// ---------------------------------------------------------------------------

/// Which account, and which of its Pages, a Meta command acts on.
#[cfg(feature = "meta")]
#[derive(Args, Clone)]
struct MetaTarget {
    /// Account alias
    #[arg(long)]
    account: String,

    /// Page ID, exact name, or unique name fragment (optional when the account
    /// has a single Page)
    #[arg(long)]
    page: Option<String>,
}

#[cfg(feature = "meta")]
#[derive(Subcommand)]
enum MetaCommands {
    /// Configure the Meta app credentials (App ID + App Secret)
    Setup {
        /// Meta app ID
        #[arg(long)]
        app_id: String,
        /// Meta app secret
        #[arg(long)]
        app_secret: String,
        /// Facebook Login for Business configuration ID (App Dashboard →
        /// Facebook Login for Business → Configurations)
        #[arg(long)]
        config_id: Option<String>,
    },

    /// Authenticate a Meta account and discover its Pages
    Auth {
        /// Account alias (e.g. "z29k")
        #[arg(long)]
        account: String,
        /// Port for the local OAuth callback server
        #[arg(long, default_value_t = crate::core::oauth::DEFAULT_PORT)]
        port: u16,
        /// Override the redirect URI (e.g. an HTTPS tunnel for a Live app)
        #[arg(long)]
        redirect_uri: Option<String>,
        /// Comma-separated scopes to request instead of the defaults
        #[arg(long)]
        scopes: Option<String>,
        /// Login for Business configuration ID, overriding the stored one
        #[arg(long)]
        config_id: Option<String>,
        /// Print the authorization URL instead of opening a browser
        #[arg(long)]
        no_browser: bool,
    },

    /// Adopt an existing access token, e.g. a Business system user token
    Token {
        /// Account alias (e.g. "z29k")
        #[arg(long)]
        account: String,
        /// The access token. Omit to read it from stdin, which keeps it out of
        /// your shell history
        #[arg(long)]
        token: Option<String>,
    },

    /// List the configured Meta accounts
    Accounts,

    /// Show Meta token status
    Status {
        /// Also ask Meta to validate each token (network call)
        #[arg(long)]
        check: bool,
    },

    /// List the Facebook Pages and linked Instagram accounts of an account
    Pages {
        /// Account alias
        #[arg(long)]
        account: String,
        /// Re-fetch Pages from Meta instead of reading the stored ones
        #[arg(long)]
        refresh: bool,
    },

    /// Set (or clear) the default Page of an account
    Use {
        /// Account alias
        #[arg(long)]
        account: String,
        /// Page ID or name; omit to clear the default
        #[arg(long)]
        page: Option<String>,
    },

    /// Raw Graph API GET (debug escape hatch)
    Get {
        #[command(flatten)]
        target: MetaTarget,
        /// Graph path, e.g. "/me" or "/123456/feed"
        #[arg(long)]
        path: String,
        /// Extra query parameter as key=value; repeatable
        #[arg(long)]
        query: Vec<String>,
    },
}

// ---------------------------------------------------------------------------
// Facebook
// ---------------------------------------------------------------------------

#[cfg(feature = "meta")]
#[derive(Subcommand)]
enum FacebookCommands {
    /// Publish a text and/or link post on a Page
    Post {
        #[command(flatten)]
        target: MetaTarget,
        /// Post text
        #[arg(long)]
        message: Option<String>,
        /// URL to attach as a link preview
        #[arg(long)]
        link: Option<String>,
        /// Unix timestamp or ISO 8601 date (10 min to 75 days ahead)
        #[arg(long)]
        schedule: Option<String>,
        /// Create the post unpublished (draft)
        #[arg(long)]
        draft: bool,
    },

    /// Publish one or more photos on a Page
    Photo {
        #[command(flatten)]
        target: MetaTarget,
        /// Local path or public URL; repeat for a multi-photo post
        #[arg(long, required = true)]
        image: Vec<String>,
        /// Post text (caption for a single photo)
        #[arg(long)]
        message: Option<String>,
        /// Unix timestamp or ISO 8601 date (10 min to 75 days ahead)
        #[arg(long)]
        schedule: Option<String>,
        /// Create the post unpublished (draft)
        #[arg(long)]
        draft: bool,
    },

    /// Publish a video on a Page
    Video {
        #[command(flatten)]
        target: MetaTarget,
        /// Local path or public URL
        #[arg(long)]
        video: String,
        /// Video title
        #[arg(long)]
        title: Option<String>,
        /// Video description
        #[arg(long)]
        description: Option<String>,
        /// Unix timestamp or ISO 8601 date (10 min to 75 days ahead)
        #[arg(long)]
        schedule: Option<String>,
        /// Create the video unpublished (draft)
        #[arg(long)]
        draft: bool,
    },
}

// ---------------------------------------------------------------------------
// Instagram
// ---------------------------------------------------------------------------

/// Options every Instagram publishing command shares.
#[cfg(feature = "meta")]
#[derive(Args, Clone)]
struct IgCommon {
    /// Caption (max 2200 characters)
    #[arg(long)]
    caption: Option<String>,

    /// Force the media kind when the extension is ambiguous: "image" or "video"
    #[arg(long)]
    media_type: Option<String>,

    /// Alt text for accessibility (images only)
    #[arg(long)]
    alt_text: Option<String>,

    /// Facebook location Page ID to tag
    #[arg(long)]
    location_id: Option<String>,

    /// Comma-separated Instagram usernames to invite as collaborators
    #[arg(long)]
    collaborators: Option<String>,

    /// Public URL of a custom video cover image
    #[arg(long)]
    cover_url: Option<String>,

    /// Milliseconds into the video to use as the thumbnail
    #[arg(long)]
    thumb_offset: Option<u64>,

    /// Flag the post as AI-generated content
    #[arg(long)]
    ai_generated: bool,

    /// Delete the temporary Facebook photos used to relay local images
    #[arg(long)]
    cleanup_relay: bool,
}

#[cfg(feature = "meta")]
#[derive(Subcommand)]
enum InstagramCommands {
    /// Publish a feed post; two or more --media make a carousel
    #[command(alias = "carousel")]
    Post {
        #[command(flatten)]
        target: MetaTarget,
        /// Local path or public URL; repeat (2-10) for a carousel
        #[arg(long, required = true)]
        media: Vec<String>,
        #[command(flatten)]
        common: IgCommon,
    },

    /// Publish a Reel (video only)
    Reel {
        #[command(flatten)]
        target: MetaTarget,
        /// Local path or public URL of the video
        #[arg(long)]
        media: String,
        #[command(flatten)]
        common: IgCommon,
    },

    /// Publish a Story
    Story {
        #[command(flatten)]
        target: MetaTarget,
        /// Local path or public URL
        #[arg(long)]
        media: String,
        /// Also share the Story to the main feed
        #[arg(long)]
        share_to_feed: bool,
        #[command(flatten)]
        common: IgCommon,
    },

    /// Publish a container that was already created
    Publish {
        #[command(flatten)]
        target: MetaTarget,
        /// Container ID returned by a previous, interrupted publish
        #[arg(long)]
        creation_id: String,
    },

    /// Show the content publishing quota for the rolling 24 h window
    Limit {
        #[command(flatten)]
        target: MetaTarget,
    },
}

#[cfg(feature = "meta")]
impl IgCommon {
    fn to_args<'a>(
        &'a self,
        target: &'a MetaTarget,
        media: &'a [String],
        surface: providers::meta::instagram::Surface,
        share_to_feed: Option<bool>,
    ) -> providers::meta::commands::IgPostArgs<'a> {
        providers::meta::commands::IgPostArgs {
            account: &target.account,
            page: target.page.as_deref(),
            media,
            media_type: self.media_type.as_deref(),
            surface,
            caption: self.caption.as_deref(),
            alt_text: self.alt_text.as_deref(),
            location_id: self.location_id.as_deref(),
            collaborators: self.collaborators.as_deref(),
            cover_url: self.cover_url.as_deref(),
            thumb_offset: self.thumb_offset,
            share_to_feed,
            ai_generated: self.ai_generated,
            cleanup_relay: self.cleanup_relay,
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    core::config::init_path(cli.config.as_deref());
    #[cfg(feature = "meta")]
    providers::meta::api::init_version(cli.api_version.as_deref());
    #[cfg(not(feature = "meta"))]
    let _ = &cli.api_version;

    if let Err(e) = run(cli.command).await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn run(command: Commands) -> Result<(), String> {
    match command {
        Commands::Accounts => commands::cmd_accounts(),
        Commands::Status { check } => commands::cmd_status(check).await,
        Commands::Migrate { from, force } => commands::cmd_migrate(&from, force),
        Commands::Broadcast(args) => {
            broadcast::run(broadcast::BroadcastArgs {
                targets: &args.targets,
                content: broadcast::Content {
                    text: args.text,
                    image: args.image,
                    link: args.link,
                    title: args.title,
                    visibility: args.visibility.to_uppercase(),
                    schedule: args.schedule,
                    alt_text: args.alt_text,
                    cleanup_relay: args.cleanup_relay,
                },
                dry_run: args.dry_run,
            })
            .await
        }

        #[cfg(feature = "linkedin")]
        Commands::Linkedin(cmd) => run_linkedin(cmd).await,
        #[cfg(feature = "meta")]
        Commands::Meta(cmd) => run_meta(cmd).await,
        #[cfg(feature = "meta")]
        Commands::Facebook(cmd) => run_facebook(cmd).await,
        #[cfg(feature = "meta")]
        Commands::Instagram(cmd) => run_instagram(cmd).await,
    }
}

#[cfg(feature = "linkedin")]
async fn run_linkedin(command: LinkedinCommands) -> Result<(), String> {
    use providers::linkedin::{auth, commands as li};

    match command {
        LinkedinCommands::Setup {
            app,
            client_id,
            client_secret,
        } => li::cmd_setup(&app.to_lowercase(), &client_id, &client_secret),

        LinkedinCommands::Auth {
            account,
            org_id,
            port,
        } => auth::run_auth(&account, org_id.as_deref(), port).await,

        LinkedinCommands::Accounts => li::cmd_accounts(),
        LinkedinCommands::Status => li::cmd_status(),

        LinkedinCommands::Post {
            account,
            text,
            visibility,
        } => li::cmd_post(&account, &text, &visibility.to_uppercase()).await,

        LinkedinCommands::Reshare {
            post_id,
            accounts,
            commentary,
            visibility,
        } => {
            li::cmd_reshare(
                &post_id,
                &accounts,
                commentary.as_deref(),
                &visibility.to_uppercase(),
            )
            .await
        }

        LinkedinCommands::Share {
            account,
            url,
            title,
            description,
            commentary,
            visibility,
            image,
            mode,
        } => {
            li::cmd_share(li::ShareArgs {
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

        LinkedinCommands::GetPost { account, post_id } => {
            li::cmd_get_post(&account, &post_id).await
        }
        LinkedinCommands::Comments {
            account,
            post_id,
            count,
            start,
        } => li::cmd_comments(&account, &post_id, count, start).await,
        LinkedinCommands::Profile { account, urn } => {
            li::cmd_profile(&account, urn.as_deref()).await
        }
    }
}

#[cfg(feature = "meta")]
async fn run_meta(command: MetaCommands) -> Result<(), String> {
    use providers::meta::{auth, commands as meta};

    match command {
        MetaCommands::Setup {
            app_id,
            app_secret,
            config_id,
        } => meta::cmd_setup(&app_id, &app_secret, config_id.as_deref()),

        MetaCommands::Auth {
            account,
            port,
            redirect_uri,
            scopes,
            config_id,
            no_browser,
        } => {
            auth::run_auth(auth::AuthArgs {
                alias: &account,
                port,
                redirect_uri: redirect_uri.as_deref(),
                scopes: scopes.as_deref(),
                config_id: config_id.as_deref(),
                no_browser,
            })
            .await
        }

        MetaCommands::Token { account, token } => meta::cmd_token(&account, token).await,
        MetaCommands::Accounts => meta::cmd_accounts(),
        MetaCommands::Status { check } => meta::cmd_status(check).await,
        MetaCommands::Pages { account, refresh } => meta::cmd_pages(&account, refresh).await,
        MetaCommands::Use { account, page } => meta::cmd_use(&account, page.as_deref()),
        MetaCommands::Get {
            target,
            path,
            query,
        } => meta::cmd_get(&target.account, target.page.as_deref(), &path, &query).await,
    }
}

#[cfg(feature = "meta")]
async fn run_facebook(command: FacebookCommands) -> Result<(), String> {
    use providers::meta::commands as meta;

    match command {
        FacebookCommands::Post {
            target,
            message,
            link,
            schedule,
            draft,
        } => {
            meta::cmd_fb_post(meta::FbPostArgs {
                account: &target.account,
                page: target.page.as_deref(),
                message: message.as_deref(),
                link: link.as_deref(),
                schedule: schedule.as_deref(),
                draft,
            })
            .await
        }

        FacebookCommands::Photo {
            target,
            image,
            message,
            schedule,
            draft,
        } => {
            meta::cmd_fb_photo(meta::FbPhotoArgs {
                account: &target.account,
                page: target.page.as_deref(),
                images: &image,
                message: message.as_deref(),
                schedule: schedule.as_deref(),
                draft,
            })
            .await
        }

        FacebookCommands::Video {
            target,
            video,
            title,
            description,
            schedule,
            draft,
        } => {
            meta::cmd_fb_video(meta::FbVideoArgs {
                account: &target.account,
                page: target.page.as_deref(),
                video: &video,
                title: title.as_deref(),
                description: description.as_deref(),
                schedule: schedule.as_deref(),
                draft,
            })
            .await
        }
    }
}

#[cfg(feature = "meta")]
async fn run_instagram(command: InstagramCommands) -> Result<(), String> {
    use providers::meta::commands as meta;
    use providers::meta::instagram::Surface;

    match command {
        InstagramCommands::Post {
            target,
            media,
            common,
        } => meta::cmd_ig_post(common.to_args(&target, &media, Surface::Feed, None)).await,

        InstagramCommands::Reel {
            target,
            media,
            common,
        } => {
            let media = [media];
            meta::cmd_ig_post(common.to_args(&target, &media, Surface::Reels, None)).await
        }

        InstagramCommands::Story {
            target,
            media,
            share_to_feed,
            common,
        } => {
            let media = [media];
            meta::cmd_ig_post(common.to_args(
                &target,
                &media,
                Surface::Stories,
                share_to_feed.then_some(true),
            ))
            .await
        }

        InstagramCommands::Publish {
            target,
            creation_id,
        } => meta::cmd_ig_publish(&target.account, target.page.as_deref(), &creation_id).await,

        InstagramCommands::Limit { target } => {
            meta::cmd_ig_limit(&target.account, target.page.as_deref()).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
