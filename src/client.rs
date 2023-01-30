use cached::proc_macro::cached;
use futures_lite::{future::BoxedLocal, FutureExt};
use js_sys::Promise;
use percent_encoding::{percent_encode, CONTROLS};
use serde_json::Value;
use std::result::Result;
use wasm_bindgen::prelude::*;
use web_sys::{Headers, Request, RequestInit, RequestRedirect, Response, ResponseInit, Url};

use crate::{
	server::RequestExt,
	utils::{promise, wasm_error},
};

#[wasm_bindgen]
extern "C" {
	#[wasm_bindgen(js_name = fetch)]
	fn fetch_with_request(input: &Request) -> Promise;
}

const REDDIT_URL_BASE: &str = "https://www.reddit.com";

/// Gets the canonical path for a resource on Reddit. This is accomplished by
/// making a `HEAD` request to Reddit at the path given in `path`.
///
/// This function returns `Ok(Some(path))`, where `path`'s value is identical
/// to that of the value of the argument `path`, if Reddit responds to our
/// `HEAD` request with a 2xx-family HTTP code. It will also return an
/// `Ok(Some(String))` if Reddit responds to our `HEAD` request with a
/// `Location` header in the response, and the HTTP code is in the 3xx-family;
/// the `String` will contain the path as reported in `Location`. The return
/// value is `Ok(None)` if Reddit responded with a 3xx, but did not provide a
/// `Location` header. An `Err(String)` is returned if Reddit responds with a
/// 429, or if we were unable to decode the value in the `Location` header.
#[cached(size = 1024, time = 600, result = true)]
pub async fn canonical_path(path: String) -> Result<Option<String>, String> {
	let res = reddit_head(path.clone(), true).await?;

	if res.status() == 429 {
		return Err("Too many requests.".to_string());
	};

	// If Reddit responds with a 2xx, then the path is already canonical.
	if res.status().to_string().starts_with('2') {
		return Ok(Some(path));
	}

	// If Reddit responds with anything other than 3xx (except for the 2xx as
	// above), return a None.
	if !res.status().to_string().starts_with('3') {
		return Ok(None);
	}

	Ok(res.headers().get("location").ok().map(|val| {
		percent_encode(val.unwrap_or_default().as_bytes(), CONTROLS)
			.to_string()
			.trim_start_matches(REDDIT_URL_BASE)
			.to_string()
	}))
}

pub async fn proxy(req: Request, format: &str) -> Result<Response, String> {
	let mut url = format!("{}{}", format, req.uri().search());

	// For each parameter in request
	req.params().for_each(&mut |value, key| {
		let name = key.as_string().unwrap_or_default();
		let value = &value.as_string().unwrap_or_default();
		url = url.replace(&format!("{{{}}}", name), value);
	});

	stream(&url, &req).await
}

async fn stream(url: &str, req: &Request) -> Result<Response, String> {
	// First parameter is target URL (mandatory).
	Url::new(url).map_err(|_| "Couldn't parse URL".to_string())?;

	// Build the hyper client from the HTTPS connector.

	let mut req_init = RequestInit::new();
	let headers = Headers::new().unwrap();

	// Copy useful headers from original request
	for &key in &["Range", "If-Modified-Since", "Cache-Control"] {
		if let Some(value) = req.headers().get(key).ok().flatten() {
			headers.set(key, &value).ok();
		}
	}

	req_init.headers(&headers);

	let req = Request::new_with_str_and_init(url, &req_init).unwrap();

	let response: Response = promise(fetch_with_request(&req)).await?;

	let headers = response.headers();

	let rm = |key: &str| headers.delete(key).ok();
	rm("access-control-expose-headers");
	rm("server");
	rm("vary");
	rm("etag");
	rm("x-cdn");
	rm("x-cdn-client-region");
	rm("x-cdn-name");
	rm("x-cdn-server-region");
	rm("x-reddit-cdn");
	rm("x-reddit-video-features");

	let mut init = ResponseInit::new();
	init.status(response.status());
	init.status_text(&response.status_text());
	init.headers(&headers);

	Response::new_with_opt_readable_stream_and_init(response.body().as_ref(), &init).map_err(wasm_error)
}

/// Makes a GET request to Reddit at `path`. By default, this will honor HTTP
/// 3xx codes Reddit returns and will automatically redirect.
fn reddit_get(path: String, quarantine: bool) -> BoxedLocal<Result<Response, String>> {
	request("GET", path, true, quarantine)
}

/// Makes a HEAD request to Reddit at `path`. This will not follow redirects.
fn reddit_head(path: String, quarantine: bool) -> BoxedLocal<Result<Response, String>> {
	request("HEAD", path, false, quarantine)
}

/// Makes a request to Reddit. If `redirect` is `true`, request_with_redirect
/// will recurse on the URL that Reddit provides in the Location HTTP header
/// in its response.
fn request(method: &'static str, path: String, redirect: bool, quarantine: bool) -> BoxedLocal<Result<Response, String>> {
	// Build Reddit URL from path.
	let url = format!("{}{}", REDDIT_URL_BASE, path);

	let headers = Headers::new().unwrap();

	headers.set("User-Agent", &format!("web:libreddit:{}", env!("CARGO_PKG_VERSION"))).ok();
	headers.set("Host", "www.reddit.com").ok();
	headers.set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8").ok();
	headers.set("Accept-Encoding", if method == "GET" { "gzip" } else { "identity" }).ok();
	headers.set("Accept-Language", "en-US,en;q=0.5").ok();
	headers.set("Connection", "keep-alive").ok();
	headers
		.set("Cookie", if quarantine { "_options=%7B%22pref_quarantine_optin%22%3A%20true%7D" } else { "" })
		.ok();

	let mut req = RequestInit::new();
	req.method(method);
	req.redirect(if redirect { RequestRedirect::Follow } else { RequestRedirect::Manual });

	let fut = async move {
		let req = Request::new_with_str_and_init(&url, &req).map_err(wasm_error)?;

		promise(fetch_with_request(&req)).await
	};

	fut.boxed_local()
}

// Make a request to a Reddit API and parse the JSON response
#[cached(size = 100, time = 30, result = true)]
pub async fn json(path: String, quarantine: bool) -> Result<Value, String> {
	// Closure to quickly build errors
	let err = |msg: &str, e: String| -> String {
		// eprintln!("{} - {}: {}", url, msg, e);
		format!("{}: {}", msg, e)
	};

	match reddit_get(path.clone(), quarantine)
		.await
		.map_err(|e| err("Couldn't send request to Reddit", e))
		.and_then(|res| {
			if res.status() >= 500 {
				Err("Reddit is having issues, check if there's an outage".to_string())
			} else {
				Ok(res)
			}
		})?
		.json()
	{
		Ok(p) => {
			let json = promise::<JsValue>(p).await?;
			let json: Value = serde_wasm_bindgen::from_value(json).unwrap_or_default();
			if json["error"].is_i64() {
				Err(
					json["reason"]
						.as_str()
						.unwrap_or_else(|| {
							json["message"].as_str().unwrap_or_else(|| {
								eprintln!("{}{} - Error parsing reddit error", REDDIT_URL_BASE, path);
								"Error parsing reddit error"
							})
						})
						.to_string(),
				)
			} else {
				Ok(json)
			}
		}
		Err(err) => Err(wasm_error(err)),
	}
}
