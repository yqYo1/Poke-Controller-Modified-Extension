//! Acceptance tests for the Web-primary/Tauri-adapter boundary.

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::io;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use axum::Router;
    use axum::http::StatusCode;
    use axum::routing::get;
    use syn::visit::Visit as _;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;
    use tokio::time::timeout;

    use crate::camera::ScreenshotMode;
    use crate::server::static_files::StaticFiles;
    use crate::{UiCapabilities, UiMode, ui_router};

    const DESKTOP_ADAPTER_DEPENDENCIES: [&str; 4] =
        ["opener", "rfd", "tauri", "tauri_plugin_single_instance"];

    async fn acceptance_server(mode: UiMode) -> (TempDir, SocketAddr, JoinHandle<io::Result<()>>) {
        let root = tempfile::tempdir().expect("isolated Web root");
        fs::write(root.path().join("index.html"), "primary-spa").expect("Web entrypoint");
        let static_files = StaticFiles::new(root.path()).expect("valid Web root");
        let api = Router::new().route("/api/capability", get(|| async { "primary-api" }));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("acceptance listener");
        let address = listener.local_addr().expect("acceptance address");
        let app = ui_router(api, static_files, address, mode.capabilities());
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        (root, address, server)
    }

    async fn http_get(
        address: SocketAddr,
        path: &str,
        origin: Option<&str>,
    ) -> (StatusCode, String) {
        timeout(Duration::from_secs(2), async move {
            let mut stream = tokio::net::TcpStream::connect(address)
                .await
                .expect("acceptance connection");
            let origin_header = origin
                .map(|value| format!("Origin: {value}\r\n"))
                .unwrap_or_default();
            let request = format!(
                "GET {path} HTTP/1.1\r\nHost: {address}\r\n{origin_header}Connection: close\r\n\r\n"
            );
            stream
                .write_all(request.as_bytes())
                .await
                .expect("acceptance request");
            let mut response = Vec::new();
            stream
                .read_to_end(&mut response)
                .await
                .expect("acceptance response");
            let response = String::from_utf8(response).expect("UTF-8 acceptance response");
            let (head, body) = response
                .split_once("\r\n\r\n")
                .expect("HTTP response boundary");
            let status = head
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<u16>().ok())
                .and_then(|value| StatusCode::from_u16(value).ok())
                .expect("HTTP response status");
            (status, body.to_owned())
        })
        .await
        .expect("acceptance request deadline")
    }

    #[test]
    fn ar_11_11_mode_capability_matrix_is_exact() {
        assert_eq!(
            [
                (UiMode::Web, UiMode::Web.capabilities()),
                (UiMode::Desktop, UiMode::Desktop.capabilities()),
            ],
            [
                (
                    UiMode::Web,
                    UiCapabilities {
                        allow_tauri_origin: false,
                        screenshot_mode: ScreenshotMode::Web,
                    },
                ),
                (
                    UiMode::Desktop,
                    UiCapabilities {
                        allow_tauri_origin: true,
                        screenshot_mode: ScreenshotMode::Desktop,
                    },
                ),
            ]
        );
    }

    #[tokio::test]
    async fn ar_11_11_both_modes_use_the_same_primary_spa_and_api() {
        for mode in [UiMode::Web, UiMode::Desktop] {
            let (_root, address, server) = acceptance_server(mode).await;
            for (path, expected_body) in [("/", "primary-spa"), ("/api/capability", "primary-api")]
            {
                let (status, body) = http_get(address, path, None).await;
                assert_eq!(status, StatusCode::OK, "{mode:?} {path}");
                assert_eq!(body, expected_body, "{mode:?} {path}");
            }

            let (tauri_status, _body) =
                http_get(address, "/api/capability", Some("tauri://localhost")).await;
            let expected_status = if mode.capabilities().allow_tauri_origin {
                StatusCode::OK
            } else {
                StatusCode::FORBIDDEN
            };
            assert_eq!(tauri_status, expected_status, "{mode:?}");

            server.abort();
            let error = server
                .await
                .expect_err("acceptance server must remain active until cleanup");
            assert!(error.is_cancelled());
        }
    }

    fn rust_sources(root: &Path, sources: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(root).expect("Rust source directory") {
            let entry = entry.expect("Rust source entry");
            let file_type = entry.file_type().expect("Rust source file type");
            let path = entry.path();
            if file_type.is_dir() {
                rust_sources(&path, sources);
            } else if file_type.is_file()
                && path.extension().is_some_and(|extension| extension == "rs")
            {
                sources.push(path);
            }
        }
    }

    #[derive(Default)]
    struct DependencyVisitor {
        dependencies: BTreeSet<String>,
    }

    impl DependencyVisitor {
        fn record_identifier(&mut self, identifier: &syn::Ident) {
            let name = identifier.to_string();
            let name = name.strip_prefix("r#").unwrap_or(&name);
            if DESKTOP_ADAPTER_DEPENDENCIES.contains(&name) {
                self.dependencies.insert(name.to_owned());
            }
        }

        fn record_use_root(&mut self, tree: &syn::UseTree) {
            match tree {
                syn::UseTree::Path(path) => self.record_identifier(&path.ident),
                syn::UseTree::Name(name) => self.record_identifier(&name.ident),
                syn::UseTree::Rename(rename) => self.record_identifier(&rename.ident),
                syn::UseTree::Group(group) => {
                    for item in &group.items {
                        self.record_use_root(item);
                    }
                }
                syn::UseTree::Glob(_) => {}
            }
        }

        fn record_token_stream(&mut self, tokens: proc_macro2::TokenStream) {
            for token in tokens {
                match token {
                    proc_macro2::TokenTree::Group(group) => {
                        self.record_token_stream(group.stream());
                    }
                    proc_macro2::TokenTree::Ident(identifier) => {
                        self.record_identifier(&identifier);
                    }
                    proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => {}
                }
            }
        }
    }

    impl<'ast> syn::visit::Visit<'ast> for DependencyVisitor {
        fn visit_path(&mut self, path: &'ast syn::Path) {
            if let Some(segment) = path.segments.first() {
                self.record_identifier(&segment.ident);
            }
            syn::visit::visit_path(self, path);
        }

        fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
            self.record_identifier(&item.ident);
            syn::visit::visit_item_extern_crate(self, item);
        }

        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            self.record_use_root(&item.tree);
            syn::visit::visit_item_use(self, item);
        }

        fn visit_macro(&mut self, item: &'ast syn::Macro) {
            self.record_token_stream(item.tokens.clone());
            syn::visit::visit_macro(self, item);
        }

        fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
            if let syn::Meta::List(list) = &attribute.meta {
                self.record_token_stream(list.tokens.clone());
            }
            syn::visit::visit_attribute(self, attribute);
        }
    }

    fn dependencies_in(source: &str) -> BTreeSet<String> {
        let syntax = syn::parse_file(source).expect("valid Rust source");
        let mut visitor = DependencyVisitor::default();
        visitor.visit_file(&syntax);
        visitor.dependencies
    }

    #[test]
    fn ar_11_11_dependency_parser_covers_aliases_and_macro_tokens() {
        let dependencies = dependencies_in(
            r#"
            use ::tauri as desktop_runtime;

            #[adapter(opener::open)]
            fn adapter() {
                identity!(rfd::FileDialog::new());
            }

            macro_rules! install_plugin {
                () => { tauri_plugin_single_instance::init(|_, _, _| {}) };
            }

            const TEXT: &str = "tauri::Builder and rfd::FileDialog";
            // opener::open and tauri_plugin_single_instance::init
            "#,
        );

        assert_eq!(
            dependencies,
            BTreeSet::from(DESKTOP_ADAPTER_DEPENDENCIES.map(str::to_owned),)
        );
        assert!(
            dependencies_in(
                r#"
                const TEXT: &str = "tauri::Builder";
                // rfd::FileDialog and opener::open
                "#,
            )
            .is_empty()
        );
    }

    #[test]
    fn ar_11_11_native_dependencies_stay_in_the_desktop_adapter_boundary() {
        let manifest_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let source_root = manifest_root.join("src");
        let mut sources = Vec::new();
        rust_sources(&source_root, &mut sources);
        sources.sort();

        let mut dependency_owners = BTreeMap::<String, BTreeSet<String>>::new();
        for source_path in sources {
            let source = fs::read_to_string(&source_path).expect("UTF-8 Rust source");
            let dependencies = dependencies_in(&source);
            let relative = source_path
                .strip_prefix(&manifest_root)
                .expect("source below manifest root")
                .to_string_lossy()
                .replace('\\', "/");
            for dependency in dependencies {
                dependency_owners
                    .entry(dependency)
                    .or_default()
                    .insert(relative.clone());
            }
        }

        assert_eq!(
            dependency_owners,
            BTreeMap::from([
                (
                    "opener".to_owned(),
                    BTreeSet::from(["src/desktop/mod.rs".to_owned()]),
                ),
                (
                    "rfd".to_owned(),
                    BTreeSet::from(["src/desktop/mod.rs".to_owned()]),
                ),
                (
                    "tauri".to_owned(),
                    BTreeSet::from([
                        "src/desktop/mod.rs".to_owned(),
                        "src/entrypoint.rs".to_owned(),
                    ]),
                ),
                (
                    "tauri_plugin_single_instance".to_owned(),
                    BTreeSet::from(["src/desktop/mod.rs".to_owned()]),
                ),
            ])
        );
    }
}
