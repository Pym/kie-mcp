use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use reqwest::Client;
use serde::Serialize;
use serde_json::{Map, Value};
use tokio::{
    sync::{Mutex, OnceCell},
    task::JoinSet,
};
use tracing::warn;
use url::Url;

use crate::kie::{KieError, jobs::GenerationKind, operations::StructuredOperation};

use super::{
    ModelSpec, OutputFormatStyle, PromptPolicy, UrlBinding, model_catalog, models_for,
    normalize_key, resolve_model_any_kind, validation,
};

const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;
const MAX_CONCURRENT_ROUTE_REQUESTS: usize = 8;

#[derive(Debug, Clone)]
pub struct LiveCatalog {
    inner: Arc<LiveCatalogInner>,
}

#[derive(Debug)]
struct LiveCatalogInner {
    http: Client,
    source: Url,
    index: OnceCell<IndexLoad>,
    routes: Mutex<HashMap<String, Arc<OnceCell<RouteLoad>>>>,
}

#[derive(Debug, Clone)]
struct IndexLoad {
    routes: Vec<RouteIndex>,
    warning: Option<String>,
}

#[derive(Debug, Clone)]
struct RouteLoad {
    contract: Option<Arc<ModelContract>>,
    warning: Option<String>,
}

#[derive(Debug, Clone)]
struct RouteIndex {
    order: usize,
    display_name: String,
    kind: GenerationKind,
    url: Url,
}

#[derive(Debug, Clone)]
pub struct ModelContract {
    pub id: String,
    pub display_name: String,
    pub kind: GenerationKind,
    pub documentation_url: String,
    pub authoritative: bool,
    order: usize,
    input_schema: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogEntry {
    pub id: String,
    pub display_name: String,
    pub kind: GenerationKind,
    pub prompt_policy: PromptPolicy,
    pub url_binding: CatalogUrlBinding,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio_field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_field: Option<String>,
    pub output_format: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    pub catalog_source: CatalogEntrySource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_url: Option<String>,
    pub schema_status: SchemaStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CatalogUrlBinding {
    None,
    Scalar {
        field: String,
    },
    Array {
        field: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_items: Option<usize>,
    },
    FirstLastFrame {
        first_field: String,
        last_field: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogEntrySource {
    LiveOpenapi,
    EmbeddedFallback,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaStatus {
    Authoritative,
    Informational,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogStatus {
    Live,
    LivePartial,
    EmbeddedFallback,
}

#[derive(Debug, Clone)]
pub struct CatalogListing {
    pub source: String,
    pub status: CatalogStatus,
    pub routes_discovered: usize,
    pub schemas_loaded: usize,
    pub schema_failures: usize,
    pub warning: Option<String>,
    pub models: Vec<CatalogEntry>,
}

impl LiveCatalog {
    pub fn new(source: &str, http_timeout: Duration) -> Self {
        let source = Url::parse(source).expect("catalog URL was validated by Config");
        let timeout = http_timeout.min(Duration::from_secs(30));
        let redirect_origin = source.clone();
        let http = Client::builder()
            .timeout(timeout)
            .user_agent(concat!("kie-mcp/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 10 {
                    attempt.error("too many catalog redirects")
                } else if same_origin(&redirect_origin, attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("catalog redirect changed origin")
                }
            }))
            .build()
            .expect("valid catalog HTTP client configuration");
        Self {
            inner: Arc::new(LiveCatalogInner {
                http,
                source,
                index: OnceCell::new(),
                routes: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn source(&self) -> &str {
        self.inner.source.as_str()
    }

    pub async fn models(
        &self,
        kind: Option<GenerationKind>,
        query: Option<&str>,
        include_descriptions: bool,
    ) -> CatalogListing {
        let embedded = models_for(kind, query);
        let index = self.index().await;
        if index.routes.is_empty() {
            return CatalogListing {
                source: self.source().to_string(),
                status: CatalogStatus::EmbeddedFallback,
                routes_discovered: 0,
                schemas_loaded: 0,
                schema_failures: 0,
                warning: index.warning.clone(),
                models: embedded
                    .into_iter()
                    .map(CatalogEntry::from_embedded)
                    .collect(),
            };
        }

        let candidates = route_candidates(&index.routes, kind, query, &embedded);
        let candidate_count = candidates.len();
        let (contracts, schema_failures) = self.load_contracts(candidates).await;
        let mut entries = contracts
            .iter()
            .filter(|contract| StructuredOperation::from_model(&contract.id).is_none())
            .map(|contract| CatalogEntry::from_contract(contract, include_descriptions))
            .collect::<Vec<_>>();

        let mut seen = entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<HashSet<_>>();
        for spec in embedded {
            if seen.insert(spec.id.to_string()) {
                entries.push(CatalogEntry::from_embedded(spec));
            }
        }
        entries = apply_entry_query(entries, kind, query);

        let warning = (schema_failures > 0).then(|| {
            format!(
                "{schema_failures} of {candidate_count} matching KIE route schema(s) could not be loaded; embedded entries remain available where known"
            )
        });
        CatalogListing {
            source: self.source().to_string(),
            status: if schema_failures == 0 {
                CatalogStatus::Live
            } else {
                CatalogStatus::LivePartial
            },
            routes_discovered: index.routes.len(),
            schemas_loaded: contracts.len(),
            schema_failures,
            warning: warning.or_else(|| index.warning.clone()),
            models: entries,
        }
    }

    pub async fn resolve_contract(
        &self,
        requested: &str,
        expected: Option<GenerationKind>,
    ) -> Option<Arc<ModelContract>> {
        let index = self.index().await;
        if index.routes.is_empty() {
            return None;
        }

        let embedded = resolve_model_any_kind(requested)
            .filter(|spec| expected.is_none_or(|kind| spec.kind == kind));
        let embedded_matches = embedded.into_iter().collect::<Vec<_>>();
        let candidates =
            route_candidates(&index.routes, expected, Some(requested), &embedded_matches);
        let (contracts, _) = self.load_contracts(candidates).await;
        let canonical = embedded.map(|spec| normalize_key(spec.id));
        if let Some(canonical) = canonical {
            return contracts
                .into_iter()
                .find(|contract| normalize_key(&contract.id) == canonical);
        }

        let requested = normalize_key(requested);
        let exact = contracts
            .iter()
            .filter(|contract| {
                normalize_key(&contract.id) == requested
                    || normalize_key(&contract.display_name) == requested
            })
            .cloned()
            .collect::<Vec<_>>();
        if exact.len() == 1 {
            return exact.into_iter().next();
        }
        if !exact.is_empty() {
            return None;
        }

        let fuzzy = contracts
            .into_iter()
            .filter(|contract| {
                normalize_key(&contract.id).contains(&requested)
                    || normalize_key(&contract.display_name).contains(&requested)
            })
            .collect::<Vec<_>>();
        (fuzzy.len() == 1)
            .then(|| fuzzy.into_iter().next())
            .flatten()
    }

    async fn index(&self) -> &IndexLoad {
        self.inner
            .index
            .get_or_init(|| async { self.fetch_index().await })
            .await
    }

    async fn fetch_index(&self) -> IndexLoad {
        let result = async {
            let response = self
                .inner
                .http
                .get(self.inner.source.clone())
                .send()
                .await
                .map_err(|err| err.to_string())?
                .error_for_status()
                .map_err(|err| err.to_string())?;
            let bytes = response.bytes().await.map_err(|err| err.to_string())?;
            if bytes.len() > MAX_CATALOG_BYTES {
                return Err(format!(
                    "catalog index exceeds the {MAX_CATALOG_BYTES}-byte safety limit"
                ));
            }
            let text = std::str::from_utf8(&bytes)
                .map_err(|err| format!("catalog index is not UTF-8: {err}"))?;
            let routes = parse_catalog_index(&self.inner.source, text);
            if routes.is_empty() {
                return Err(
                    "catalog index did not contain KIE Market image or video routes".into(),
                );
            }
            Ok::<_, String>(routes)
        }
        .await;

        match result {
            Ok(routes) => IndexLoad {
                routes,
                warning: None,
            },
            Err(message) => {
                warn!(error = %message, source = %self.inner.source, "using embedded KIE catalog fallback");
                IndexLoad {
                    routes: Vec::new(),
                    warning: Some(format!("live KIE catalog unavailable: {message}")),
                }
            }
        }
    }

    async fn load_contracts(&self, routes: Vec<RouteIndex>) -> (Vec<Arc<ModelContract>>, usize) {
        let mut tasks = JoinSet::new();
        let mut contracts = Vec::new();
        let mut failures = 0;

        for route in routes {
            let catalog = self.clone();
            tasks.spawn(async move { catalog.load_route(route).await });
            if tasks.len() >= MAX_CONCURRENT_ROUTE_REQUESTS
                && let Some(result) = tasks.join_next().await
            {
                collect_route_result(result, &mut contracts, &mut failures);
            }
        }
        while let Some(result) = tasks.join_next().await {
            collect_route_result(result, &mut contracts, &mut failures);
        }

        contracts.sort_by_key(|contract| contract.order);
        let mut seen = HashSet::new();
        contracts.retain(|contract| seen.insert(contract.id.clone()));
        (contracts, failures)
    }

    async fn load_route(&self, route: RouteIndex) -> RouteLoad {
        let key = route.url.as_str().to_string();
        let cell = {
            let mut routes = self.inner.routes.lock().await;
            routes
                .entry(key)
                .or_insert_with(|| Arc::new(OnceCell::new()))
                .clone()
        };
        cell.get_or_init(|| async { self.fetch_route(route).await })
            .await
            .clone()
    }

    async fn fetch_route(&self, route: RouteIndex) -> RouteLoad {
        let result = async {
            let response = self
                .inner
                .http
                .get(route.url.clone())
                .send()
                .await
                .map_err(|err| err.to_string())?
                .error_for_status()
                .map_err(|err| err.to_string())?;
            let bytes = response.bytes().await.map_err(|err| err.to_string())?;
            if bytes.len() > MAX_CATALOG_BYTES {
                return Err(format!(
                    "route document exceeds the {MAX_CATALOG_BYTES}-byte safety limit"
                ));
            }
            let markdown = std::str::from_utf8(&bytes)
                .map_err(|err| format!("route document is not UTF-8: {err}"))?;
            parse_route_contract(&route, markdown)
        }
        .await;

        match result {
            Ok(Some(contract)) => RouteLoad {
                contract: Some(Arc::new(contract)),
                warning: None,
            },
            Ok(None) => RouteLoad {
                contract: None,
                warning: None,
            },
            Err(message) => {
                warn!(error = %message, route = %route.url, "KIE route schema unavailable");
                RouteLoad {
                    contract: None,
                    warning: Some(message),
                }
            }
        }
    }
}

impl ModelContract {
    pub fn validate(&self, input: &Value) -> Result<(), KieError> {
        if !self.authoritative {
            return Ok(());
        }
        validation::validate_input(&self.input_schema, input).map_err(|message| {
            KieError::InvalidRequest {
                message: format!(
                    "{}: {message} according to {}",
                    self.id, self.documentation_url
                ),
            }
        })
    }

    fn schema_for_output(&self, include_descriptions: bool) -> Value {
        sanitize_schema(&self.input_schema, include_descriptions)
    }
}

impl CatalogEntry {
    fn from_embedded(spec: &'static ModelSpec) -> Self {
        Self {
            id: spec.id.to_string(),
            display_name: spec.display_name.to_string(),
            kind: spec.kind,
            prompt_policy: spec.prompt_policy,
            url_binding: CatalogUrlBinding::from(spec.url_binding),
            aspect_ratio_field: spec.aspect_ratio_field.map(str::to_string),
            resolution_field: spec.resolution_field.map(str::to_string),
            output_format: output_format_name(spec.output_format).to_string(),
            aliases: spec.aliases.iter().map(|alias| alias.to_string()).collect(),
            catalog_source: CatalogEntrySource::EmbeddedFallback,
            documentation_url: None,
            schema_status: SchemaStatus::Unavailable,
            input_schema: None,
        }
    }

    fn from_contract(contract: &ModelContract, include_descriptions: bool) -> Self {
        let embedded = model_catalog().iter().find(|spec| spec.id == contract.id);
        let (
            prompt_policy,
            url_binding,
            aspect_ratio_field,
            resolution_field,
            output_format,
            aliases,
        ) = if let Some(spec) = embedded {
            (
                spec.prompt_policy,
                CatalogUrlBinding::from(spec.url_binding),
                spec.aspect_ratio_field.map(str::to_string),
                spec.resolution_field.map(str::to_string),
                output_format_name(spec.output_format).to_string(),
                spec.aliases.iter().map(|alias| alias.to_string()).collect(),
            )
        } else {
            (
                prompt_policy(&contract.input_schema),
                CatalogUrlBinding::None,
                None,
                None,
                "none".to_string(),
                Vec::new(),
            )
        };
        Self {
            id: contract.id.clone(),
            display_name: contract.display_name.clone(),
            kind: contract.kind,
            prompt_policy,
            url_binding,
            aspect_ratio_field,
            resolution_field,
            output_format,
            aliases,
            catalog_source: CatalogEntrySource::LiveOpenapi,
            documentation_url: Some(contract.documentation_url.clone()),
            schema_status: if contract.authoritative {
                SchemaStatus::Authoritative
            } else {
                SchemaStatus::Informational
            },
            input_schema: Some(contract.schema_for_output(include_descriptions)),
        }
    }

    pub fn prompt_summary(&self) -> &'static str {
        match self.prompt_policy {
            PromptPolicy::Required => "required",
            PromptPolicy::Optional => "optional",
            PromptPolicy::None => "none",
        }
    }

    pub fn input_summary(&self) -> String {
        match &self.url_binding {
            CatalogUrlBinding::None => "model-specific input".to_string(),
            CatalogUrlBinding::Scalar { field } | CatalogUrlBinding::Array { field, .. } => {
                field.clone()
            }
            CatalogUrlBinding::FirstLastFrame {
                first_field,
                last_field,
            } => format!("{first_field}/{last_field}"),
        }
    }

    pub fn convenience_summary(&self) -> Vec<&str> {
        let mut fields = Vec::new();
        if let Some(field) = self.aspect_ratio_field.as_deref() {
            fields.push(field);
        }
        if let Some(field) = self.resolution_field.as_deref() {
            fields.push(field);
        }
        if self.output_format != "none" {
            fields.push("output_format");
        }
        fields
    }
}

impl From<UrlBinding> for CatalogUrlBinding {
    fn from(value: UrlBinding) -> Self {
        match value {
            UrlBinding::None => Self::None,
            UrlBinding::Scalar { field } => Self::Scalar {
                field: field.to_string(),
            },
            UrlBinding::Array { field, max_items } => Self::Array {
                field: field.to_string(),
                max_items,
            },
            UrlBinding::FirstLastFrame {
                first_field,
                last_field,
            } => Self::FirstLastFrame {
                first_field: first_field.to_string(),
                last_field: last_field.to_string(),
            },
        }
    }
}

fn collect_route_result(
    result: Result<RouteLoad, tokio::task::JoinError>,
    contracts: &mut Vec<Arc<ModelContract>>,
    failures: &mut usize,
) {
    match result {
        Ok(RouteLoad {
            contract: Some(contract),
            ..
        }) => contracts.push(contract),
        Ok(RouteLoad {
            warning: Some(_), ..
        }) => {
            *failures += 1;
        }
        Ok(RouteLoad { .. }) => {}
        Err(err) => {
            warn!(error = %err, "KIE catalog route task failed");
            *failures += 1;
        }
    }
}

fn route_candidates(
    routes: &[RouteIndex],
    kind: Option<GenerationKind>,
    query: Option<&str>,
    embedded: &[&ModelSpec],
) -> Vec<RouteIndex> {
    let query = query.map(normalize_key).filter(|query| !query.is_empty());
    let embedded_names = embedded
        .iter()
        .map(|spec| normalize_key(spec.display_name))
        .collect::<HashSet<_>>();
    routes
        .iter()
        .filter(|route| kind.is_none_or(|kind| route.kind == kind))
        .filter(|route| {
            let Some(query) = query.as_deref() else {
                return true;
            };
            let title = normalize_key(&route.display_name);
            title.contains(query)
                || normalize_key(route.url.path()).contains(query)
                || embedded_names.contains(&title)
        })
        .cloned()
        .collect()
}

fn apply_entry_query(
    entries: Vec<CatalogEntry>,
    kind: Option<GenerationKind>,
    query: Option<&str>,
) -> Vec<CatalogEntry> {
    let mut entries = entries
        .into_iter()
        .filter(|entry| kind.is_none_or(|kind| entry.kind == kind))
        .collect::<Vec<_>>();
    let Some(query) = query.map(normalize_key).filter(|query| !query.is_empty()) else {
        return entries;
    };
    let exact = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry_exact_match(entry, &query))
        .map(|(index, _)| index)
        .collect::<HashSet<_>>();
    if !exact.is_empty() {
        return entries
            .into_iter()
            .enumerate()
            .filter(|(index, _)| exact.contains(index))
            .map(|(_, entry)| entry)
            .collect();
    }
    entries.retain(|entry| entry_contains_match(entry, &query));
    entries
}

fn entry_exact_match(entry: &CatalogEntry, query: &str) -> bool {
    normalize_key(&entry.id) == query
        || normalize_key(&entry.display_name) == query
        || entry
            .aliases
            .iter()
            .any(|alias| normalize_key(alias) == query)
}

fn entry_contains_match(entry: &CatalogEntry, query: &str) -> bool {
    normalize_key(&entry.id).contains(query)
        || normalize_key(&entry.display_name).contains(query)
        || entry
            .aliases
            .iter()
            .any(|alias| normalize_key(alias).contains(query))
}

fn parse_catalog_index(source: &Url, text: &str) -> Vec<RouteIndex> {
    let mut routes = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        let kind = if line.starts_with("- Image") {
            GenerationKind::Image
        } else if line.starts_with("- Video") {
            GenerationKind::Video
        } else {
            continue;
        };
        let Some(title_start) = line.find('[') else {
            continue;
        };
        let Some(title_end_offset) = line[title_start + 1..].find("](") else {
            continue;
        };
        let title_end = title_start + 1 + title_end_offset;
        let url_start = title_end + 2;
        let Some(url_end_offset) = line[url_start..].find(')') else {
            continue;
        };
        let url_end = url_start + url_end_offset;
        let display_name = line[title_start + 1..title_end].trim();
        let href = &line[url_start..url_end];
        let Ok(url) = source.join(href) else {
            continue;
        };
        if !same_origin(source, &url)
            || url.path().contains("/cn/")
            || !url.path().contains("/market/")
            || !url.path().ends_with(".md")
            || display_name.is_empty()
            || !seen.insert(url.as_str().to_string())
        {
            continue;
        }
        routes.push(RouteIndex {
            order: routes.len(),
            display_name: display_name.to_string(),
            kind,
            url,
        });
    }
    routes
}

fn parse_route_contract(
    route: &RouteIndex,
    markdown: &str,
) -> Result<Option<ModelContract>, String> {
    let yaml = extract_openapi_yaml(markdown)
        .ok_or_else(|| "route document does not contain a fenced OpenAPI YAML block".to_string())?;
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(yaml).map_err(|err| format!("invalid OpenAPI YAML: {err}"))?;
    let document = serde_json::to_value(yaml)
        .map_err(|err| format!("OpenAPI YAML cannot be represented as JSON: {err}"))?;
    if !document
        .get("openapi")
        .and_then(Value::as_str)
        .is_some_and(|version| version.starts_with("3."))
    {
        return Err("route document is not OpenAPI 3.x".to_string());
    }
    let Some(post) = document.pointer("/paths/~1api~1v1~1jobs~1createTask/post") else {
        return Ok(None);
    };
    let request_body = post
        .get("requestBody")
        .ok_or_else(|| "createTask route does not define a request body".to_string())?;
    let request_body = dereference(&document, request_body, &mut Vec::new());
    let content = request_body
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| "createTask request body does not define content".to_string())?;
    let media = content
        .iter()
        .find(|(media_type, _)| media_type.starts_with("application/json"))
        .map(|(_, media)| media)
        .ok_or_else(|| "createTask request body does not define application/json".to_string())?;
    let request_schema = media
        .get("schema")
        .ok_or_else(|| "createTask JSON body does not define a schema".to_string())?;
    let request_schema = dereference(&document, request_schema, &mut Vec::new());
    let properties = request_schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| "createTask request schema does not define properties".to_string())?;
    let model_schema = properties
        .get("model")
        .ok_or_else(|| "createTask request schema does not define model".to_string())?;
    let model_schema = dereference(&document, model_schema, &mut Vec::new());
    let (id, model_is_exact) = exact_model_id(&model_schema)
        .ok_or_else(|| "createTask model schema does not identify one model".to_string())?;
    if !route_identifies_model(route, &id) {
        return Err(format!("route identity conflicts with declared model {id}"));
    }
    let input_schema = properties
        .get("input")
        .ok_or_else(|| "createTask request schema does not define input".to_string())?;
    let input_schema = dereference(&document, input_schema, &mut Vec::new());
    let input_is_object = input_schema.get("type").and_then(Value::as_str) == Some("object")
        || input_schema.get("properties").is_some()
        || input_schema.get("oneOf").is_some()
        || input_schema.get("anyOf").is_some();
    if !input_is_object {
        return Err("createTask input schema is not object-shaped".to_string());
    }

    Ok(Some(ModelContract {
        id,
        display_name: route.display_name.clone(),
        kind: route.kind,
        documentation_url: route.url.as_str().to_string(),
        authoritative: model_is_exact && input_is_object,
        order: route.order,
        input_schema,
    }))
}

fn extract_openapi_yaml(markdown: &str) -> Option<&str> {
    let marker = "```yaml";
    let start = markdown.find(marker)? + marker.len();
    let tail = markdown
        .get(start..)?
        .strip_prefix('\r')
        .unwrap_or(&markdown[start..]);
    let tail = tail.strip_prefix('\n').unwrap_or(tail);
    let end = tail.find("\n```\n").or_else(|| tail.find("\n```\r\n"))?;
    tail.get(..end)
}

fn exact_model_id(schema: &Value) -> Option<(String, bool)> {
    if let Some(value) = schema.get("const").and_then(Value::as_str) {
        return Some((value.to_string(), true));
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array)
        && values.len() == 1
        && let Some(value) = values[0].as_str()
    {
        return Some((value.to_string(), true));
    }
    schema
        .get("default")
        .and_then(Value::as_str)
        .map(|value| (value.to_string(), false))
}

fn route_identifies_model(route: &RouteIndex, id: &str) -> bool {
    let id_key = normalize_key(id);
    let path_key = normalize_key(route.url.path());
    let title_key = normalize_key(&route.display_name);
    if !id_key.is_empty() && (path_key.contains(&id_key) || title_key.contains(&id_key)) {
        return true;
    }

    let id_tail = id.rsplit('/').next().map(normalize_key).unwrap_or_default();
    let route_slug = route
        .url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .and_then(|segment| segment.strip_suffix(".md"))
        .map(normalize_key)
        .unwrap_or_default();
    if !id_tail.is_empty()
        && ((id_tail == route_slug)
            || (route_slug.len() >= 8
                && (id_tail.contains(&route_slug) || route_slug.contains(&id_tail)))
            || (id_tail.len() >= 8 && title_key.contains(&id_tail)))
    {
        return true;
    }

    model_catalog()
        .iter()
        .find(|spec| spec.id == id)
        .is_some_and(|spec| {
            normalize_key(spec.display_name) == title_key
                || spec
                    .aliases
                    .iter()
                    .any(|alias| normalize_key(alias) == title_key)
        })
}

fn dereference(document: &Value, value: &Value, stack: &mut Vec<String>) -> Value {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str)
                && let Some(pointer) = reference.strip_prefix('#')
                && !stack.iter().any(|seen| seen == reference)
                && let Some(target) = document.pointer(pointer)
            {
                stack.push(reference.to_string());
                let mut resolved = dereference(document, target, stack);
                stack.pop();
                if let Some(resolved) = resolved.as_object_mut() {
                    for (key, value) in object.iter().filter(|(key, _)| *key != "$ref") {
                        resolved.insert(key.clone(), dereference(document, value, stack));
                    }
                }
                return resolved;
            }
            Value::Object(
                object
                    .iter()
                    .map(|(key, value)| (key.clone(), dereference(document, value, stack)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| dereference(document, value, stack))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn sanitize_schema(value: &Value, include_descriptions: bool) -> Value {
    match value {
        Value::Object(object) => {
            let mut sanitized = Map::new();
            for (key, value) in object {
                if matches!(
                    key.as_str(),
                    "properties"
                        | "$defs"
                        | "definitions"
                        | "patternProperties"
                        | "dependentSchemas"
                ) && let Some(properties) = value.as_object()
                {
                    sanitized.insert(
                        key.clone(),
                        Value::Object(
                            properties
                                .iter()
                                .map(|(name, schema)| {
                                    (name.clone(), sanitize_schema(schema, include_descriptions))
                                })
                                .collect(),
                        ),
                    );
                    continue;
                }
                if key.starts_with("x-") || matches!(key.as_str(), "example" | "examples") {
                    continue;
                }
                if key == "description" && !include_descriptions {
                    continue;
                }
                sanitized.insert(key.clone(), sanitize_schema(value, include_descriptions));
            }
            Value::Object(sanitized)
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| sanitize_schema(value, include_descriptions))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn prompt_policy(schema: &Value) -> PromptPolicy {
    if schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| required.iter().any(|field| field == "prompt"))
    {
        PromptPolicy::Required
    } else if schema_contains_property(schema, "prompt") {
        PromptPolicy::Optional
    } else {
        PromptPolicy::None
    }
}

fn schema_contains_property(schema: &Value, field: &str) -> bool {
    match schema {
        Value::Object(object) => {
            object
                .get("properties")
                .and_then(Value::as_object)
                .is_some_and(|properties| properties.contains_key(field))
                || object
                    .values()
                    .any(|value| schema_contains_property(value, field))
        }
        Value::Array(values) => values
            .iter()
            .any(|value| schema_contains_property(value, field)),
        _ => false,
    }
}

fn output_format_name(style: OutputFormatStyle) -> &'static str {
    match style {
        OutputFormatStyle::None => "none",
        OutputFormatStyle::Jpg => "jpg_png",
        OutputFormatStyle::Jpeg => "jpeg_png",
    }
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const INDEX: &str = r#"
## API Docs
- Image Models > Example [Example Image](https://docs.example.test/market/example-image.md): image
- Video Models > Example [Example Video](/market/example-video.md): video
- Music Models > Example [Example Audio](https://docs.example.test/market/audio.md): audio
- Image Models > Legacy [Legacy](https://docs.example.test/legacy.md): legacy
- Image Models > Chinese [Chinese](https://docs.example.test/cn/market/example.md): translated
- Image Models > External [External](https://other.example.test/market/example.md): external
"#;

    const ROUTE: &str = r#"
# Example

## OpenAPI Specification

```yaml
openapi: 3.0.1
paths:
  /api/v1/jobs/createTask:
    post:
      description: |-
        Example request:
        ```json
        {"model":"example/image"}
        ```
      requestBody:
        content:
          application/json:
            schema:
              type: object
              required: [model, input]
              properties:
                model:
                  type: string
                  enum: [example/image]
                input:
                  $ref: '#/components/schemas/Input'
components:
  schemas:
    Input:
      type: object
      required: [prompt, mode]
      properties:
        prompt:
          type: string
          maxLength: 120
          description: Prompt text
          examples: [ignored]
        mode:
          type: string
          enum: [std, pro]
          x-apidog-enum: []
        shots:
          type: array
          maxItems: 2
          items:
            type: object
            required: [duration]
            properties:
              duration:
                type: integer
                minimum: 1
                maximum: 5
```
"#;

    #[test]
    fn index_keeps_only_same_origin_market_image_and_video_routes() {
        let source = Url::parse("https://docs.example.test/llms.txt").unwrap();
        let routes = parse_catalog_index(&source, INDEX);

        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].display_name, "Example Image");
        assert_eq!(routes[0].kind, GenerationKind::Image);
        assert_eq!(
            routes[1].url.as_str(),
            "https://docs.example.test/market/example-video.md"
        );
    }

    #[test]
    fn route_parser_dereferences_and_preserves_recursive_constraints() {
        let route = RouteIndex {
            order: 0,
            display_name: "Example Image".to_string(),
            kind: GenerationKind::Image,
            url: Url::parse("https://docs.example.test/market/example-image.md").unwrap(),
        };
        let contract = parse_route_contract(&route, ROUTE).unwrap().unwrap();

        assert_eq!(contract.id, "example/image");
        assert!(contract.authoritative);
        assert_eq!(contract.input_schema["required"], json!(["prompt", "mode"]));
        assert_eq!(
            contract.input_schema["properties"]["mode"]["enum"],
            json!(["std", "pro"])
        );
        assert_eq!(
            contract.input_schema["properties"]["shots"]["items"]["properties"]["duration"]["maximum"],
            5
        );

        let public = contract.schema_for_output(false);
        assert_eq!(public["properties"]["prompt"]["maxLength"], 120);
        assert!(public["properties"]["prompt"].get("description").is_none());
        assert!(public["properties"]["prompt"].get("examples").is_none());
        assert!(public["properties"]["mode"].get("x-apidog-enum").is_none());
        assert_eq!(
            contract.schema_for_output(true)["properties"]["prompt"]["description"],
            "Prompt text"
        );
    }

    #[test]
    fn route_contract_rejects_invalid_nested_input_locally() {
        let route = RouteIndex {
            order: 0,
            display_name: "Example Image".to_string(),
            kind: GenerationKind::Image,
            url: Url::parse("https://docs.example.test/market/example-image.md").unwrap(),
        };
        let contract = parse_route_contract(&route, ROUTE).unwrap().unwrap();
        let error = contract
            .validate(&json!({
                "prompt": "hello",
                "mode": "ultra",
                "shots": [{ "duration": 6 }]
            }))
            .unwrap_err();

        assert!(error.to_string().contains("input.mode must be one of"));
        assert!(error.to_string().contains("example-image.md"));
    }

    #[test]
    fn informational_contract_does_not_reject_input() {
        let contract = ModelContract {
            id: "example/image".to_string(),
            display_name: "Example".to_string(),
            kind: GenerationKind::Image,
            documentation_url: "https://docs.example.test/model.md".to_string(),
            authoritative: false,
            order: 0,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "mode": { "type": "string", "enum": ["std", "pro"] }
                }
            }),
        };

        assert!(contract.validate(&json!({ "mode": "unknown" })).is_ok());
    }

    #[test]
    fn route_parser_rejects_a_model_id_from_another_route() {
        let route = RouteIndex {
            order: 0,
            display_name: "Example Image".to_string(),
            kind: GenerationKind::Image,
            url: Url::parse("https://docs.example.test/market/example-image.md").unwrap(),
        };
        let mismatched = ROUTE.replace("example/image", "other/video-model");

        let error = parse_route_contract(&route, &mismatched).unwrap_err();
        assert!(error.contains("route identity conflicts"));
        assert!(error.contains("other/video-model"));
    }

    #[test]
    fn route_parser_accepts_a_renamed_route_confirmed_by_its_title() {
        let route = RouteIndex {
            order: 0,
            display_name: "Other Video Model".to_string(),
            kind: GenerationKind::Video,
            url: Url::parse("https://docs.example.test/market/legacy-page.md").unwrap(),
        };
        let renamed = ROUTE.replace("example/image", "other/video-model");

        let contract = parse_route_contract(&route, &renamed).unwrap().unwrap();
        assert_eq!(contract.id, "other/video-model");
        assert!(contract.authoritative);
    }
}
