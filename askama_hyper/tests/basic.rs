use std::convert::Infallible;

use askama::Template;
use hyper::body::to_bytes;
use hyper::http::response::Builder;
use hyper::{Body, Client, Request, Response, Server};
use routerify::ext::RequestExt;
use routerify::{Router, RouterService};

#[derive(Template)]
#[template(path = "hello.html")]
struct HelloTemplate<'a, N>
where
    N: std::fmt::Display,
{
    name: &'a N,
}

async fn hello_handler(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let name = req.param("name").unwrap();
    let template = &HelloTemplate { name: &name };
    Ok(template.into())
}

async fn try_body_handler(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let name = req.param("name").unwrap();
    let template = &HelloTemplate { name: &name };
    Ok(Builder::new().body(template.try_into().unwrap()).unwrap())
}

fn router() -> Router<Body, Infallible> {
    Router::builder()
        .get("/hello/:name", hello_handler)
        .get("/try_body/:name", try_body_handler)
        .build()
        .unwrap()
}

#[tokio::test]
async fn test_hyper() {
    let addr = ([127, 0, 0, 1], 0).into();
    let service = RouterService::new(router()).expect("Could not create service");
    let server = Server::bind(&addr).serve(service);
    let local_addr = server.local_addr();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    let serve = async move {
        let server = server.with_graceful_shutdown(async { while rx.recv().await.is_some() {} });
        server.await.expect("Could not serve");
    };
    let hello_tx = tx.clone();
    let hello_query = async move {
        let uri = format!("http://{local_addr}/hello/world")
            .parse()
            .expect("Could not format URI");
        let client = Client::new();

        let res = client.get(uri).await.expect("Could not query client");
        assert_eq!(res.status(), hyper::StatusCode::OK);

        let content_type = res
            .headers()
            .get("content-type")
            .expect("Response did not contain content-type header")
            .to_str()
            .expect("Content-type was not a UTF-8 string");
        assert_eq!(content_type, mime::TEXT_HTML_UTF_8.to_string());

        let body = to_bytes(res).await.expect("No body returned");
        let body = std::str::from_utf8(&body).expect("Body was not UTF-8");
        assert_eq!(body, "Hello, world!");

        hello_tx.send(()).unwrap();
    };

    let try_body_query = async move {
        let uri = format!("http://{local_addr}/try_body/world")
            .parse()
            .expect("Could not format URI");
        let client = Client::new();

        let res = client.get(uri).await.expect("Could not query client");
        assert_eq!(res.status(), hyper::StatusCode::OK);

        let body = to_bytes(res).await.expect("No body returned");
        let body = std::str::from_utf8(&body).expect("Body was not UTF-8");
        assert_eq!(body, "Hello, world!");

        tx.send(()).unwrap();
    };

    tokio::join!(serve, try_body_query, hello_query);
}
