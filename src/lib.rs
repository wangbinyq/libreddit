// Global specifiers
#![forbid(unsafe_code)]
#![allow(clippy::cmp_owned)]

// Reference local files
mod config;
mod duplicates;
mod post;
mod search;
mod settings;
mod subreddit;
mod user;
mod utils;

use std::panic;

// Import Crates
use futures_lite::FutureExt;

mod client;
use client::{canonical_path, proxy};
use js_sys::Uint8Array;
use once_cell::sync::Lazy;
use server::{RequestExt, Server};
use utils::{error, redirect, wasm_error, ThemeAssets};
use wasm_bindgen::prelude::*;
use web_sys::{Request, Response};

mod server;

// Create Services

// Required for the manifest to be valid
async fn pwa_logo() -> Result<Response, String> {
	resource(include_bytes!("../static/logo.png").as_ref(), "image/png", false).await
}

// Required for iOS App Icons
async fn iphone_logo() -> Result<Response, String> {
	resource(include_bytes!("../static/apple-touch-icon.png").as_ref(), "image/png", false).await
}

async fn favicon() -> Result<Response, String> {
	resource(include_bytes!("../static/favicon.ico").as_ref(), "image/vnd.microsoft.icon", true).await
}

async fn font() -> Result<Response, String> {
	resource(include_bytes!("../static/Inter.var.woff2").as_ref(), "font/woff2", true).await
}

async fn resource(body: impl AsRef<[u8]>, content_type: &str, cache: bool) -> Result<Response, String> {
	let body: Uint8Array = body.as_ref().into();
	let response = Response::new_with_opt_buffer_source(Some(&body)).map_err(wasm_error)?;
	response.headers().set("content-type", content_type).ok();

	if cache {
		response.headers().set("Cache-Control", "public, max-age=1209600, s-maxage=86400").ok();
	}

	Ok(response)
}

async fn style() -> Result<Response, String> {
	let mut res = include_str!("../static/style.css").to_string();
	for file in ThemeAssets::iter() {
		res.push('\n');
		let theme = ThemeAssets::get(file.as_ref()).unwrap();
		res.push_str(std::str::from_utf8(theme.data.as_ref()).unwrap());
	}

	resource(&res, "text/css", true).await
}

static SERVER: Lazy<Server> = Lazy::new(|| {
	panic::set_hook(Box::new(console_error_panic_hook::hook));
	println!("Starting Libreddit...");

	// Begin constructing a server
	let mut app = server::Server::new();

	// Define default headers (added to all responses)
	app.default_headers = headers! {
		"Referrer-Policy" => "no-referrer",
		"X-Content-Type-Options" => "nosniff",
		"X-Frame-Options" => "DENY",
		"Content-Security-Policy" => "default-src 'none'; font-src 'self'; script-src 'self' blob:; manifest-src 'self'; media-src 'self' data: blob: about:; style-src 'self' 'unsafe-inline'; base-uri 'none'; img-src 'self' data:; form-action 'self'; frame-ancestors 'none'; connect-src 'self'; worker-src blob:;"
	};

	// Read static files
	app.at("/style.css").get(|_| style().boxed_local());
	app
		.at("/manifest.json")
		.get(|_| resource(include_str!("../static/manifest.json"), "application/json", false).boxed_local());
	app
		.at("/robots.txt")
		.get(|_| resource("User-agent: *\nDisallow: /u/\nDisallow: /user/", "text/plain", true).boxed_local());
	app.at("/favicon.ico").get(|_| favicon().boxed_local());
	app.at("/logo.png").get(|_| pwa_logo().boxed_local());
	app.at("/Inter.var.woff2").get(|_| font().boxed_local());
	app.at("/touch-icon-iphone.png").get(|_| iphone_logo().boxed_local());
	app.at("/apple-touch-icon.png").get(|_| iphone_logo().boxed_local());
	app
		.at("/playHLSVideo.js")
		.get(|_| resource(include_str!("../static/playHLSVideo.js"), "text/javascript", false).boxed_local());
	app
		.at("/hls.min.js")
		.get(|_| resource(include_str!("../static/hls.min.js"), "text/javascript", false).boxed_local());

	// Proxy media through Libreddit
	app.at("/vid/:id/:size").get(|r| proxy(r, "https://v.redd.it/{id}/DASH_{size}").boxed_local());
	app.at("/hls/:id/*path").get(|r| proxy(r, "https://v.redd.it/{id}/{path}").boxed_local());
	app.at("/img/*path").get(|r| proxy(r, "https://i.redd.it/{path}").boxed_local());
	app.at("/thumb/:point/:id").get(|r| proxy(r, "https://{point}.thumbs.redditmedia.com/{id}").boxed_local());
	app.at("/emoji/:id/:name").get(|r| proxy(r, "https://emoji.redditmedia.com/{id}/{name}").boxed_local());
	app
		.at("/preview/:loc/award_images/:fullname/:id")
		.get(|r| proxy(r, "https://{loc}view.redd.it/award_images/{fullname}/{id}").boxed_local());
	app.at("/preview/:loc/:id").get(|r| proxy(r, "https://{loc}view.redd.it/{id}").boxed_local());
	app.at("/style/*path").get(|r| proxy(r, "https://styles.redditmedia.com/{path}").boxed_local());
	app.at("/static/*path").get(|r| proxy(r, "https://www.redditstatic.com/{path}").boxed_local());

	// Browse user profile
	app
		.at("/u/:name")
		.get(|r| async move { Ok(redirect(format!("/user/{}", r.param("name").unwrap_or_default()))) }.boxed_local());
	app.at("/u/:name/comments/:id/:title").get(|r| post::item(r).boxed_local());
	app.at("/u/:name/comments/:id/:title/:comment_id").get(|r| post::item(r).boxed_local());

	app.at("/user/[deleted]").get(|req| error(req, "User has deleted their account".to_string()).boxed_local());
	app.at("/user/:name").get(|r| user::profile(r).boxed_local());
	app.at("/user/:name/:listing").get(|r| user::profile(r).boxed_local());
	app.at("/user/:name/comments/:id").get(|r| post::item(r).boxed_local());
	app.at("/user/:name/comments/:id/:title").get(|r| post::item(r).boxed_local());
	app.at("/user/:name/comments/:id/:title/:comment_id").get(|r| post::item(r).boxed_local());

	// Configure settings
	app.at("/settings").get(|r| settings::get(r).boxed_local()).post(|r| settings::set(r).boxed_local());
	app.at("/settings/restore").get(|r| settings::restore(r).boxed_local());
	app.at("/settings/update").get(|r| settings::update(r).boxed_local());

	// Subreddit services
	app
		.at("/r/:sub")
		.get(|r| subreddit::community(r).boxed_local())
		.post(|r| subreddit::add_quarantine_exception(r).boxed_local());

	app
		.at("/r/u_:name")
		.get(|r| async move { Ok(redirect(format!("/user/{}", r.param("name").unwrap_or_default()))) }.boxed_local());

	app.at("/r/:sub/subscribe").post(|r| subreddit::subscriptions_filters(r).boxed_local());
	app.at("/r/:sub/unsubscribe").post(|r| subreddit::subscriptions_filters(r).boxed_local());
	app.at("/r/:sub/filter").post(|r| subreddit::subscriptions_filters(r).boxed_local());
	app.at("/r/:sub/unfilter").post(|r| subreddit::subscriptions_filters(r).boxed_local());

	app.at("/r/:sub/comments/:id").get(|r| post::item(r).boxed_local());
	app.at("/r/:sub/comments/:id/:title").get(|r| post::item(r).boxed_local());
	app.at("/r/:sub/comments/:id/:title/:comment_id").get(|r| post::item(r).boxed_local());
	app.at("/comments/:id").get(|r| post::item(r).boxed_local());
	app.at("/comments/:id/comments").get(|r| post::item(r).boxed_local());
	app.at("/comments/:id/comments/:comment_id").get(|r| post::item(r).boxed_local());
	app.at("/comments/:id/:title").get(|r| post::item(r).boxed_local());
	app.at("/comments/:id/:title/:comment_id").get(|r| post::item(r).boxed_local());

	app.at("/r/:sub/duplicates/:id").get(|r| duplicates::item(r).boxed_local());
	app.at("/r/:sub/duplicates/:id/:title").get(|r| duplicates::item(r).boxed_local());
	app.at("/duplicates/:id").get(|r| duplicates::item(r).boxed_local());
	app.at("/duplicates/:id/:title").get(|r| duplicates::item(r).boxed_local());

	app.at("/r/:sub/search").get(|r| search::find(r).boxed_local());

	app
		.at("/r/:sub/w")
		.get(|r| async move { Ok(redirect(format!("/r/{}/wiki", r.param("sub").unwrap_or_default()))) }.boxed_local());
	app
		.at("/r/:sub/w/*page")
		.get(|r| async move { Ok(redirect(format!("/r/{}/wiki/{}", r.param("sub").unwrap_or_default(), r.param("wiki").unwrap_or_default()))) }.boxed_local());
	app.at("/r/:sub/wiki").get(|r| subreddit::wiki(r).boxed_local());
	app.at("/r/:sub/wiki/*page").get(|r| subreddit::wiki(r).boxed_local());

	app.at("/r/:sub/about/sidebar").get(|r| subreddit::sidebar(r).boxed_local());

	app.at("/r/:sub/:sort").get(|r| subreddit::community(r).boxed_local());

	// Front page
	app.at("/").get(|r| subreddit::community(r).boxed_local());

	// View Reddit wiki
	app.at("/w").get(|_| async { Ok(redirect("/wiki".to_string())) }.boxed_local());
	app
		.at("/w/*page")
		.get(|r| async move { Ok(redirect(format!("/wiki/{}", r.param("page").unwrap_or_default()))) }.boxed_local());
	app.at("/wiki").get(|r| subreddit::wiki(r).boxed_local());
	app.at("/wiki/*page").get(|r| subreddit::wiki(r).boxed_local());

	// Search all of Reddit
	app.at("/search").get(|r| search::find(r).boxed_local());

	// Handle about pages
	app.at("/about").get(|req| error(req, "About pages aren't added yet".to_string()).boxed_local());

	app.at("/:id").get(|req: Request| {
		Box::pin(async move {
			match req.param("id").as_deref() {
				// Sort front page
				Some("best" | "hot" | "new" | "top" | "rising" | "controversial") => subreddit::community(req).await,

				// Short link for post
				Some(id) if (5..8).contains(&id.len()) => match canonical_path(format!("/{}", id)).await {
					Ok(path_opt) => match path_opt {
						Some(path) => Ok(redirect(path)),
						None => error(req, "Post ID is invalid. It may point to a post on a community that has been banned.").await,
					},
					Err(e) => error(req, e).await,
				},

				// Error message for unknown pages
				_ => error(req, "Nothing here".to_string()).await,
			}
		})
	});

	// Default service in case no routes match
	app.at("/*").get(|req| error(req, "Nothing here".to_string()).boxed_local());

	app
});

#[wasm_bindgen]
pub async fn serve(req: Request) -> Result<Response, String> {
	SERVER.serve(req).await
}
