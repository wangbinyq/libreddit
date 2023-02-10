// CRATES
use crate::server::{RequestExt, ResponseExt};
use crate::utils::{promise, redirect, template, wasm_error, Preferences};
use askama::Template;
use cookie::Cookie;
use time::{Duration, OffsetDateTime};
use web_sys::{FormData, Request, Response};

// STRUCTS
#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsTemplate {
	prefs: Preferences,
	url: String,
}

// CONSTANTS

const PREFS: [&str; 13] = [
	"theme",
	"front_page",
	"layout",
	"wide",
	"comment_sort",
	"post_sort",
	"show_nsfw",
	"blur_nsfw",
	"use_hls",
	"hide_hls_notification",
	"autoplay_videos",
	"hide_awards",
	"disable_visit_reddit_confirmation",
];

// FUNCTIONS

// Retrieve cookies from request "Cookie" header
pub async fn get(req: Request) -> Result<Response, String> {
	let url = req.uri().pathname();
	template(SettingsTemplate {
		prefs: Preferences::new(&req),
		url,
	})
}

// Set cookies using response "Set-Cookie" header
pub async fn set(req: Request) -> Result<Response, String> {
	// Grab existing cookies
	// let _cookies: Vec<Cookie> = parts
	// 	.headers
	// 	.get_all("Cookie")
	// 	.iter()
	// 	.filter_map(|header| Cookie::parse(header.to_str().unwrap_or_default()).ok())
	// 	.collect();

	// Aggregate the body...
	// let whole_body = reqwest::body::aggregate(req).await.map_err(|e| e.to_string())?;
	let form = promise::<FormData>(req.form_data().map_err(wasm_error)?).await?;

	let mut response = redirect("/settings".to_string());

	for &name in &PREFS {
		let data = form.get_all(name);
		match data.get(data.length() - 1).as_string() {
			Some(value) => response.insert_cookie(
				Cookie::build(name.to_owned(), value.clone())
					.path("/")
					.http_only(true)
					.expires(OffsetDateTime::now_utc() + Duration::weeks(52))
					.finish(),
			),
			None => response.remove_cookie(name.to_string()),
		};
	}

	Ok(response)
}

fn set_cookies_method(req: Request, remove_cookies: bool) -> Response {
	// Split the body into parts

	// Grab existing cookies
	// let _cookies: Vec<Cookie> = parts
	// 	.headers
	// 	.get_all("Cookie")
	// 	.iter()
	// 	.filter_map(|header| Cookie::parse(header.to_str().unwrap_or_default()).ok())
	// 	.collect();

	let form = req.uri().search_params();

	let path = match form.get("redirect") {
		Some(value) => format!("/{}", value.replace("%26", "&").replace("%23", "#")),
		None => "/".to_string(),
	};

	let mut response = redirect(path);

	for name in [PREFS.to_vec(), vec!["subscriptions", "filters"]].concat() {
		match form.get(name) {
			Some(value) => response.insert_cookie(
				Cookie::build(name.to_owned(), value.clone())
					.path("/")
					.http_only(true)
					.expires(OffsetDateTime::now_utc() + Duration::weeks(52))
					.finish(),
			),
			None => {
				if remove_cookies {
					response.remove_cookie(name.to_string());
				}
			}
		};
	}

	response
}

// Set cookies using response "Set-Cookie" header
pub async fn restore(req: Request) -> Result<Response, String> {
	Ok(set_cookies_method(req, true))
}

pub async fn update(req: Request) -> Result<Response, String> {
	Ok(set_cookies_method(req, false))
}
