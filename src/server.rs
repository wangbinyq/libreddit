use cookie::Cookie;
use futures_lite::{future::BoxedLocal, FutureExt};
use js_sys::Map;
use route_recognizer::{Params, Router};
use std::{collections::HashMap, result::Result, string::ToString};
use time::Duration;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Headers, Request, Response, ResponseInit, Url};

use crate::utils::wasm_error;

type BoxResponse = BoxedLocal<Result<Response, String>>;

pub struct Route<'a> {
	router: &'a mut Router<fn(Request) -> BoxResponse>,
	path: String,
}

pub struct Server {
	pub default_headers: HashMap<String, String>,
	router: Router<fn(Request) -> BoxResponse>,
}

#[macro_export]
macro_rules! headers(
	{ $($key:expr => $value:expr),+ } => {
		{
			let mut m = ::std::collections::HashMap::new();
			$(
					m.insert($key.to_string(), $value.to_string());
			)+
			m
		}
	 };
);

pub trait RequestExt {
	fn uri(&self) -> Url;
	fn params(&self) -> Map;
	fn param(&self, name: &str) -> Option<String>;
	fn set_params(&mut self, params: Params);
	fn cookies(&self) -> Vec<Cookie>;
	fn cookie(&self, name: &str) -> Option<Cookie>;
}

pub trait ResponseExt {
	fn cookies(&self) -> Vec<Cookie>;
	fn insert_cookie(&mut self, cookie: Cookie);
	fn remove_cookie(&mut self, name: String);
}

impl RequestExt for Request {
	fn uri(&self) -> Url {
		let url = Url::new(&self.url()).unwrap();

		if url.search().is_empty() {
			url.set_search("?_")
		}

		url
	}

	fn params(&self) -> Map {
		let params = js_sys::Reflect::get(self, &JsValue::from_str("params")).unwrap_or_default();

		JsCast::dyn_into(params).unwrap_or_default()
	}

	fn param(&self, name: &str) -> Option<String> {
		self.params().get(&JsValue::from_str(name)).as_string()
	}

	fn set_params(&mut self, params: Params) {
		let map = Map::new();
		for (key, value) in &params {
			map.set(&JsValue::from_str(key), &JsValue::from_str(value));
		}

		js_sys::Reflect::set(self, &JsValue::from_str("params"), &map).ok();
	}

	fn cookies(&self) -> Vec<Cookie> {
		self
			.headers()
			.get("Cookie")
			.ok()
			.flatten()
			.map(|header| {
				let cookies = header.split("; ");
				cookies.map(|cookie| Cookie::parse(cookie.to_string()).unwrap_or_else(|_| Cookie::named(""))).collect()
			})
			.unwrap_or_default()
	}

	fn cookie(&self, name: &str) -> Option<Cookie> {
		self.cookies().into_iter().find(|c| c.name() == name)
	}
}

impl ResponseExt for Response {
	fn cookies(&self) -> Vec<Cookie> {
		self
			.headers()
			.get("Cookie")
			.ok()
			.flatten()
			.map(|header| {
				let cookies = header.split("; ");
				cookies.map(|cookie| Cookie::parse(cookie.to_string()).unwrap_or_else(|_| Cookie::named(""))).collect()
			})
			.unwrap_or_default()
	}

	fn insert_cookie(&mut self, cookie: Cookie) {
		self.headers().append("Set-Cookie", &cookie.to_string()).ok();
	}

	fn remove_cookie(&mut self, name: String) {
		let mut cookie = Cookie::named(name);
		cookie.set_path("/");
		cookie.set_max_age(Duration::seconds(1));
		self.insert_cookie(cookie);
	}
}

impl Route<'_> {
	fn method(&mut self, method: &str, dest: fn(Request) -> BoxResponse) -> &mut Self {
		self.router.add(&format!("/{}{}", method, self.path), dest);
		self
	}

	/// Add an endpoint for `GET` requests
	pub fn get(&mut self, dest: fn(Request) -> BoxResponse) -> &mut Self {
		self.method("GET", dest)
	}

	/// Add an endpoint for `POST` requests
	pub fn post(&mut self, dest: fn(Request) -> BoxResponse) -> &mut Self {
		self.method("POST", dest)
	}
}

impl Server {
	pub fn new() -> Self {
		Server {
			default_headers: HashMap::new(),
			router: Router::new(),
		}
	}

	pub fn at(&mut self, path: &str) -> Route {
		Route {
			path: path.to_owned(),
			router: &mut self.router,
		}
	}

	pub fn serve(&self, req: Request) -> BoxedLocal<Result<Response, String>> {
		// For correct borrowing, these values need to be borrowed

		// This is the `Service` that will handle the connection.
		// `service_fn` is a helper to convert a function that
		// returns a Response into a `Service`.
		// let shared_router = router.clone();
		let req_headers = req.headers().clone();
		let def_headers = self.default_headers.clone();

		// Remove double slashes and decode encoded slashes
		let mut path = req.uri().pathname().replace("//", "/").replace("%2F", "/");

		// Remove trailing slashes
		if path != "/" && path.ends_with('/') {
			path.pop();
		}

		// Match the visited path with an added route
		match self.router.recognize(&format!("/{}{}", req.method().as_str(), path)) {
			// If a route was configured for this path
			Ok(found) => {
				let mut parammed = req;
				parammed.set_params(found.params().clone());

				// Run the route's function
				let func = (found.handler().to_owned().to_owned())(parammed);
				async move {
					match func.await {
						Ok(res) => {
							for (key, value) in def_headers {
								res.headers().set(&key, &value).ok();
							}

							Ok(res)
						}
						Err(msg) => new_boilerplate(def_headers, req_headers, 500, msg),
					}
				}
				.boxed_local()
			}
			// If there was a routing error
			Err(e) => async { new_boilerplate(def_headers, req_headers, 404, e) }.boxed_local(),
		}
	}
}

/// Create a boilerplate Response for error conditions. This response will be
/// compressed if requested by client.
fn new_boilerplate(default_headers: HashMap<String, String>, req_headers: Headers, status: u16, body: String) -> Result<Response, String> {
	for (key, value) in default_headers {
		req_headers.set(&key, &value).ok();
	}

	let mut init = ResponseInit::new();

	init.status(status);
	init.headers(&req_headers);

	Response::new_with_opt_str_and_init(Some(&body), &init).map_err(wasm_error)
}
