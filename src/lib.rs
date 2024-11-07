mod common;
mod config;
mod link;
mod proxy;

use std::sync::Arc;

use crate::config::Config;
use crate::link::generate_link;
use crate::proxy::RequestContext;

use worker::*;

lazy_static::lazy_static! {
    static ref CONFIG: Arc<Config> = {
        let c = include_str!(env!("CONFIG_PATH"));
        Arc::new(Config::new(c))
    };
}

#[event(fetch)]
async fn main(req: Request, env: Env, _: Context) -> Result<Response> {
    // 获取存储在环境变量中的密码
    let password = env.var("v2raypasswork")?.to_string();

    // 检查请求路径
    match req.path().as_str() {
        "/link" => link(req, CONFIG.clone()),
        path => match CONFIG.dispatch_inbound(path) {
            Some(inbound) => {
                // 在处理请求之前验证密码
                if let Some(response) = check_password(&req, &password).await? {
                    return Ok(response);
                }

                let context = RequestContext {
                    inbound,
                    request: Some(req),
                    ..Default::default()
                };
                tunnel(CONFIG.clone(), context).await
            }
            None => Response::empty(),
        },
    }
}

async fn check_password(req: &Request, password: &str) -> Result<Option<Response>> {
    // 检查请求中是否包含密码
    if let Some(query) = req.url()?.query() {
        let params: Vec<&str> = query.split('&').collect();
        for param in params {
            let pair: Vec<&str> = param.split('=').collect();
            if pair.len() == 2 && pair[0] == "password" && pair[1] == password {
                return Ok(None); // 密码正确，继续处理请求
            }
        }
    }

    // 如果密码不正确或未提供，返回一个要求输入密码的响应
    let html = r#"
        <html>
            <body>
                <form action="" method="get">
                    <label for="password">Enter Password:</label>
                    <input type="password" id="password" name="password">
                    <input type="submit" value="Submit">
                </form>
            </body>
        </html>
    "#;

    Ok(Some(Response::from_html(html)?))
}

async fn tunnel(config: Arc<Config>, context: RequestContext) -> Result<Response> {
    let WebSocketPair { server, client } = WebSocketPair::new()?;

    server.accept()?;
    wasm_bindgen_futures::spawn_local(async move {
        let events = server.events().unwrap();

        if let Err(e) = proxy::process(config, context, &server, events).await {
            console_log!("[tunnel]: {}", e);
        }
    });

    Response::from_websocket(client)
}

fn link(req: Request, config: Arc<Config>) -> Result<Response> {
    let host = req.url()?.host().map(|x| x.to_string()).unwrap_or_default();
    Response::from_json(&generate_link(&config, &host))
}
