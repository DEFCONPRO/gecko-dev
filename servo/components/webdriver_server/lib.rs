/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![crate_name = "webdriver_server"]
#![crate_type = "rlib"]

#![feature(rustc_private, ip_addr)]

#[macro_use]
extern crate log;

extern crate webdriver;
extern crate msg;
extern crate png;
extern crate url;
extern crate util;
extern crate rustc_serialize;
extern crate uuid;
extern crate webdriver_traits;

use msg::constellation_msg::{ConstellationChan, LoadData, PipelineId, NavigationDirection, WebDriverCommandMsg};
use msg::constellation_msg::Msg as ConstellationMsg;
use std::sync::mpsc::{channel, Receiver};
use webdriver_traits::{WebDriverScriptCommand, WebDriverJSError, WebDriverJSResult};

use url::Url;
use webdriver::command::{WebDriverMessage, WebDriverCommand};
use webdriver::command::{GetParameters, JavascriptCommandParameters, LocatorParameters, TimeoutsParameters};
use webdriver::common::{LocatorStrategy, WebElement};
use webdriver::response::{
    WebDriverResponse, NewSessionResponse, ValueResponse};
use webdriver::server::{self, WebDriverHandler, Session};
use webdriver::error::{WebDriverResult, WebDriverError, ErrorStatus};
use util::task::spawn_named;
use uuid::Uuid;

use std::borrow::ToOwned;
use rustc_serialize::json::{Json, ToJson};
use rustc_serialize::base64::{Config, ToBase64, CharacterSet, Newline};
use std::collections::BTreeMap;
use std::net::SocketAddr;

use std::thread::sleep_ms;

pub fn start_server(port: u16, constellation_chan: ConstellationChan) {
    let handler = Handler::new(constellation_chan);

    spawn_named("WebdriverHttpServer".to_owned(), move || {
        server::start(SocketAddr::new("0.0.0.0".parse().unwrap(), port), handler);
    });
}

struct WebdriverSession {
    id: Uuid
}

struct Handler {
    session: Option<WebdriverSession>,
    constellation_chan: ConstellationChan,
    script_timeout: u32,
    load_timeout: u32,
    implicit_wait_timeout: u32
}

impl WebdriverSession {
    pub fn new() -> WebdriverSession {
        WebdriverSession {
            id: Uuid::new_v4()
        }
    }
}

impl Handler {
    pub fn new(constellation_chan: ConstellationChan) -> Handler {
        Handler {
            session: None,
            constellation_chan: constellation_chan,
            script_timeout: 30_000,
            load_timeout: 300_000,
            implicit_wait_timeout: 0
        }
    }

    fn get_root_pipeline(&self) -> WebDriverResult<PipelineId> {
        let interval = 20;
        let iterations = 30_000 / interval;

        for _ in 0..iterations {
            let (sender, reciever) = channel();
            let ConstellationChan(ref const_chan) = self.constellation_chan;
            const_chan.send(ConstellationMsg::GetRootPipeline(sender)).unwrap();


            if let Some(x) = reciever.recv().unwrap() {
                return Ok(x);
            };

            sleep_ms(interval)
        };

        Err(WebDriverError::new(ErrorStatus::Timeout,
                                "Failed to get root window handle"))
    }

    fn handle_new_session(&mut self) -> WebDriverResult<WebDriverResponse> {
        if self.session.is_none() {
            let session = WebdriverSession::new();
            let mut capabilities = BTreeMap::new();
            capabilities.insert("browserName".to_owned(), "servo".to_json());
            capabilities.insert("browserVersion".to_owned(), "0.0.1".to_json());
            let rv = Ok(WebDriverResponse::NewSession(
                NewSessionResponse::new(
                    session.id.to_string(),
                    Json::Object(capabilities))));
            self.session = Some(session);
            rv
        } else {
            Err(WebDriverError::new(ErrorStatus::UnknownError,
                                    "Session already created"))
        }
    }

    fn handle_get(&self, parameters: &GetParameters) -> WebDriverResult<WebDriverResponse> {
        let url = match Url::parse(&parameters.url[..]) {
            Ok(url) => url,
            Err(_) => return Err(WebDriverError::new(ErrorStatus::InvalidArgument,
                                               "Invalid URL"))
        };

        let pipeline_id = try!(self.get_root_pipeline());

        let (sender, reciever) = channel();

        let load_data = LoadData::new(url);
        let ConstellationChan(ref const_chan) = self.constellation_chan;
        let cmd_msg = WebDriverCommandMsg::LoadUrl(pipeline_id, load_data, sender);
        const_chan.send(ConstellationMsg::WebDriverCommand(cmd_msg)).unwrap();

        //Wait to get a load event
        reciever.recv().unwrap();

        Ok(WebDriverResponse::Void)
    }

    fn handle_go_back(&self) -> WebDriverResult<WebDriverResponse> {
        let ConstellationChan(ref const_chan) = self.constellation_chan;
        const_chan.send(ConstellationMsg::Navigate(None, NavigationDirection::Back)).unwrap();
        Ok(WebDriverResponse::Void)
    }

    fn handle_go_forward(&self) -> WebDriverResult<WebDriverResponse> {
        let ConstellationChan(ref const_chan) = self.constellation_chan;
        const_chan.send(ConstellationMsg::Navigate(None, NavigationDirection::Forward)).unwrap();
        Ok(WebDriverResponse::Void)
    }

    fn handle_get_title(&self) -> WebDriverResult<WebDriverResponse> {
        let pipeline_id = try!(self.get_root_pipeline());

        let (sender, reciever) = channel();
        let ConstellationChan(ref const_chan) = self.constellation_chan;
        let cmd_msg = WebDriverCommandMsg::ScriptCommand(pipeline_id,
                                                         WebDriverScriptCommand::GetTitle(sender));
        const_chan.send(ConstellationMsg::WebDriverCommand(cmd_msg)).unwrap();
        let value = reciever.recv().unwrap();
        Ok(WebDriverResponse::Generic(ValueResponse::new(value.to_json())))
    }

    fn handle_get_window_handle(&self) -> WebDriverResult<WebDriverResponse> {
        // For now we assume there's only one window so just use the session
        // id as the window id
        let handle = self.session.as_ref().unwrap().id.to_string();
        Ok(WebDriverResponse::Generic(ValueResponse::new(handle.to_json())))
    }

    fn handle_get_window_handles(&self) -> WebDriverResult<WebDriverResponse> {
        // For now we assume there's only one window so just use the session
        // id as the window id
        let handles = vec![self.session.as_ref().unwrap().id.to_string().to_json()];
        Ok(WebDriverResponse::Generic(ValueResponse::new(handles.to_json())))
    }

    fn handle_find_element(&self, parameters: &LocatorParameters) -> WebDriverResult<WebDriverResponse> {
        let pipeline_id = try!(self.get_root_pipeline());

        if parameters.using != LocatorStrategy::CSSSelector {
            return Err(WebDriverError::new(ErrorStatus::UnsupportedOperation,
                                           "Unsupported locator strategy"))
        }

        let (sender, reciever) = channel();
        let ConstellationChan(ref const_chan) = self.constellation_chan;
        let cmd = WebDriverScriptCommand::FindElementCSS(parameters.value.clone(), sender);
        let cmd_msg = WebDriverCommandMsg::ScriptCommand(pipeline_id, cmd);
        const_chan.send(ConstellationMsg::WebDriverCommand(cmd_msg)).unwrap();
        match reciever.recv().unwrap() {
            Ok(value) => {
                Ok(WebDriverResponse::Generic(ValueResponse::new(value.map(|x| WebElement::new(x).to_json()).to_json())))
            }
            Err(_) => Err(WebDriverError::new(ErrorStatus::InvalidSelector,
                                              "Invalid selector"))
        }
    }

    fn handle_find_elements(&self, parameters: &LocatorParameters) -> WebDriverResult<WebDriverResponse> {
        let pipeline_id = try!(self.get_root_pipeline());

        if parameters.using != LocatorStrategy::CSSSelector {
            return Err(WebDriverError::new(ErrorStatus::UnsupportedOperation,
                                           "Unsupported locator strategy"))
        }

        let (sender, reciever) = channel();
        let ConstellationChan(ref const_chan) = self.constellation_chan;
        let cmd = WebDriverScriptCommand::FindElementsCSS(parameters.value.clone(), sender);
        let cmd_msg = WebDriverCommandMsg::ScriptCommand(pipeline_id, cmd);
        const_chan.send(ConstellationMsg::WebDriverCommand(cmd_msg)).unwrap();
        match reciever.recv().unwrap() {
            Ok(value) => {
                let resp_value: Vec<Json> = value.into_iter().map(
                    |x| WebElement::new(x).to_json()).collect();
                Ok(WebDriverResponse::Generic(ValueResponse::new(resp_value.to_json())))
            }
            Err(_) => Err(WebDriverError::new(ErrorStatus::InvalidSelector,
                                              "Invalid selector"))
        }
    }

    fn handle_get_element_text(&self, element: &WebElement) -> WebDriverResult<WebDriverResponse> {
        let pipeline_id = try!(self.get_root_pipeline());

        let (sender, reciever) = channel();
        let ConstellationChan(ref const_chan) = self.constellation_chan;
        let cmd = WebDriverScriptCommand::GetElementText(element.id.clone(), sender);
        let cmd_msg = WebDriverCommandMsg::ScriptCommand(pipeline_id, cmd);
        const_chan.send(ConstellationMsg::WebDriverCommand(cmd_msg)).unwrap();
        match reciever.recv().unwrap() {
            Ok(value) => Ok(WebDriverResponse::Generic(ValueResponse::new(value.to_json()))),
            Err(_) => Err(WebDriverError::new(ErrorStatus::StaleElementReference,
                                              "Unable to find element in document"))
        }
    }

    fn handle_get_active_element(&self) -> WebDriverResult<WebDriverResponse> {
        let pipeline_id = try!(self.get_root_pipeline());

        let (sender, reciever) = channel();
        let ConstellationChan(ref const_chan) = self.constellation_chan;
        let cmd = WebDriverScriptCommand::GetActiveElement(sender);
        let cmd_msg = WebDriverCommandMsg::ScriptCommand(pipeline_id, cmd);
        const_chan.send(ConstellationMsg::WebDriverCommand(cmd_msg)).unwrap();
        let value = reciever.recv().unwrap().map(|x| WebElement::new(x).to_json());
        Ok(WebDriverResponse::Generic(ValueResponse::new(value.to_json())))
    }

    fn handle_get_element_tag_name(&self, element: &WebElement) -> WebDriverResult<WebDriverResponse> {
        let pipeline_id = try!(self.get_root_pipeline());

        let (sender, reciever) = channel();
        let ConstellationChan(ref const_chan) = self.constellation_chan;
        let cmd = WebDriverScriptCommand::GetElementTagName(element.id.clone(), sender);
        let cmd_msg = WebDriverCommandMsg::ScriptCommand(pipeline_id, cmd);
        const_chan.send(ConstellationMsg::WebDriverCommand(cmd_msg)).unwrap();
        match reciever.recv().unwrap() {
            Ok(value) => Ok(WebDriverResponse::Generic(ValueResponse::new(value.to_json()))),
            Err(_) => Err(WebDriverError::new(ErrorStatus::StaleElementReference,
                                              "Unable to find element in document"))
        }
    }

    fn handle_set_timeouts(&mut self, parameters: &TimeoutsParameters) -> WebDriverResult<WebDriverResponse> {
        //TODO: this conversion is crazy, spec should limit these to u32 and check upstream
        let value = parameters.ms as u32;
        match &parameters.type_[..] {
            "implicit" => self.implicit_wait_timeout = value,
            "page load" => self.load_timeout = value,
            "script" => self.script_timeout = value,
            x @ _ => return Err(WebDriverError::new(ErrorStatus::InvalidSelector,
                                                    &format!("Unknown timeout type {}", x)))
        }
        Ok(WebDriverResponse::Void)
    }

    fn handle_execute_script(&self, parameters: &JavascriptCommandParameters) -> WebDriverResult<WebDriverResponse> {
        let func_body = &parameters.script;
        let args_string = "";

        // This is pretty ugly; we really want something that acts like
        // new Function() and then takes the resulting function and executes
        // it with a vec of arguments.
        let script = format!("(function() {{ {} }})({})", func_body, args_string);

        let (sender, reciever) = channel();
        let command = WebDriverScriptCommand::ExecuteScript(script, sender);
        self.execute_script(command, reciever)
    }

    fn handle_execute_async_script(&self, parameters: &JavascriptCommandParameters) -> WebDriverResult<WebDriverResponse> {
        let func_body = &parameters.script;
        let args_string = "window.webdriverCallback";

        let script = format!(
            "setTimeout(webdriverTimeout, {}); (function(callback) {{ {} }})({})",
            self.script_timeout, func_body, args_string);

        let (sender, reciever) = channel();
        let command = WebDriverScriptCommand::ExecuteAsyncScript(script, sender);
        self.execute_script(command, reciever)
    }

    fn execute_script(&self, command: WebDriverScriptCommand, reciever: Receiver<WebDriverJSResult>) -> WebDriverResult<WebDriverResponse> {
        // TODO: This isn't really right because it always runs the script in the
        // root window
        let pipeline_id = try!(self.get_root_pipeline());

        let ConstellationChan(ref const_chan) = self.constellation_chan;
        let cmd_msg = WebDriverCommandMsg::ScriptCommand(pipeline_id, command);
        const_chan.send(ConstellationMsg::WebDriverCommand(cmd_msg)).unwrap();

        match reciever.recv().unwrap() {
            Ok(value) => Ok(WebDriverResponse::Generic(ValueResponse::new(value.to_json()))),
            Err(WebDriverJSError::Timeout) => Err(WebDriverError::new(ErrorStatus::Timeout, "")),
            Err(WebDriverJSError::UnknownType) => Err(WebDriverError::new(
                ErrorStatus::UnsupportedOperation, "Unsupported return type"))
        }
    }

    fn handle_take_screenshot(&self) -> WebDriverResult<WebDriverResponse> {
        let mut img = None;

        let interval = 20;
        let iterations = 30_000 / interval;

        for _ in 0..iterations {
            let (sender, reciever) = channel();
            let ConstellationChan(ref const_chan) = self.constellation_chan;
            let cmd_msg = WebDriverCommandMsg::TakeScreenshot(sender);
            const_chan.send(ConstellationMsg::WebDriverCommand(cmd_msg)).unwrap();

            if let Some(x) = reciever.recv().unwrap() {
                img = Some(x);
                break;
            };

            sleep_ms(interval)
        }

        if img.is_none() {
            return Err(WebDriverError::new(ErrorStatus::Timeout,
                                           "Taking screenshot timed out"));
        }

        let img_vec = match png::to_vec(&mut img.unwrap()) {
           Ok(x) => x,
           Err(_) => return Err(WebDriverError::new(ErrorStatus::UnknownError,
                                                    "Taking screenshot failed"))
        };
        let config = Config {
            char_set:CharacterSet::Standard,
            newline: Newline::LF,
            pad: true,
            line_length: None
        };
        let encoded = img_vec.to_base64(config);
        Ok(WebDriverResponse::Generic(ValueResponse::new(encoded.to_json())))
    }
}

impl WebDriverHandler for Handler {
    fn handle_command(&mut self, _session: &Option<Session>, msg: &WebDriverMessage) -> WebDriverResult<WebDriverResponse> {

        match msg.command {
            WebDriverCommand::NewSession => self.handle_new_session(),
            WebDriverCommand::Get(ref parameters) => self.handle_get(parameters),
            WebDriverCommand::GoBack => self.handle_go_back(),
            WebDriverCommand::GoForward => self.handle_go_forward(),
            WebDriverCommand::GetTitle => self.handle_get_title(),
            WebDriverCommand::GetWindowHandle => self.handle_get_window_handle(),
            WebDriverCommand::GetWindowHandles => self.handle_get_window_handles(),
            WebDriverCommand::FindElement(ref parameters) => self.handle_find_element(parameters),
            WebDriverCommand::FindElements(ref parameters) => self.handle_find_elements(parameters),
            WebDriverCommand::GetActiveElement => self.handle_get_active_element(),
            WebDriverCommand::GetElementText(ref element) => self.handle_get_element_text(element),
            WebDriverCommand::GetElementTagName(ref element) => self.handle_get_element_tag_name(element),
            WebDriverCommand::ExecuteScript(ref x) => self.handle_execute_script(x),
            WebDriverCommand::ExecuteAsyncScript(ref x) => self.handle_execute_async_script(x),
            WebDriverCommand::SetTimeouts(ref x) => self.handle_set_timeouts(x),
            WebDriverCommand::TakeScreenshot => self.handle_take_screenshot(),
            _ => Err(WebDriverError::new(ErrorStatus::UnsupportedOperation,
                                         "Command not implemented"))
        }
    }

    fn delete_session(&mut self, _session: &Option<Session>) {
        self.session = None;
    }
}
