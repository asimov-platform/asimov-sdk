use asimov_module::Index;

const SAMPLE_INDEX: &str = r#"
{"@type":"AsimovModule","name":"anthropic","label":"Anthropic","title":"ASIMOV Anthropic Module","summary":"LLM inference powered by Anthropic.","links":["https://github.com/asimov-modules/asimov-anthropic-module","https://crates.io/crates/asimov-anthropic-module"],"provides":{"programs":["asimov-anthropic-prompter"]},"config":{"variables":[{"name":"api-key","environment":"ANTHROPIC_API_KEY"},{"name":"model","environment":"ANTHROPIC_MODEL","default":"claude-opus-4-1-20250805"}]}}
{"@type":"AsimovModule","name":"imap","label":"IMAP","title":"ASIMOV IMAP Module","summary":"IMAP email import.","links":["https://github.com/asimov-modules/asimov-imap-module","https://crates.io/crates/asimov-imap-module"],"provides":{"programs":["asimov-imap-lister","asimov-imap-fetcher"]},"handles":{"url_protocols":["imap","imaps"]},"uses":{"env_variables":["ASIMOV_IMAP_USER","ASIMOV_IMAP_PASSWORD"]}}
{"@type":"AsimovModule","name":"ipfs","label":"IPFS","title":"ASIMOV IPFS Module","summary":"IPFS protocol support.","links":["https://github.com/asimov-modules/asimov-ipfs-module","https://crates.io/crates/asimov-ipfs-module"],"provides":{"programs":["asimov-ipfs-fetcher"]},"handles":{"url_protocols":["ipfs"],"url_prefixes":null,"url_patterns":null,"file_extensions":null,"content_types":null}}
{"@type":"AsimovModule","name":"maildir","label":"Maildir","title":"ASIMOV Maildir Module","summary":"Maildir email import.","links":["https://github.com/asimov-modules/asimov-maildir-module","https://crates.io/crates/asimov-maildir-module"],"provides":{"programs":["asimov-maildir-lister","asimov-maildir-fetcher"]},"handles":{"url_protocols":["file"],"file_extensions":[".maildir"],"content_types":null}}
"#;

#[test]
fn test_parse_index() {
    let index: Index = SAMPLE_INDEX.parse().unwrap();
    assert_eq!(index.modules().len(), 4);

    let imap = &index.modules()[1];
    assert_eq!(imap.name, "imap");
    assert_eq!(imap.label.as_deref(), Some("IMAP"));
    assert_eq!(imap.title.as_deref(), Some("ASIMOV IMAP Module"));
    assert_eq!(imap.summary.as_deref(), Some("IMAP email import."));
    assert_eq!(imap.links.len(), 2);
    assert_eq!(imap.provides.programs.len(), 2);
    assert_eq!(imap.handles.url_protocols, ["imap", "imaps"]);
    assert!(imap.config.is_none());

    let variables = &index.modules()[0]
        .config
        .as_ref()
        .expect("should have config")
        .variables;
    assert_eq!(variables[0].name, "api-key");
    assert_eq!(
        variables[0].environment.as_deref(),
        Some("ANTHROPIC_API_KEY")
    );
    assert_eq!(
        variables[1].default_value.as_deref(),
        Some("claude-opus-4-1-20250805"),
    );

    assert!(index.modules()[2].handles.url_prefixes.is_empty());
}

#[test]
fn test_parse_invalid_index() {
    let error = "{\"name\":\"imap\"}\nnot json\n"
        .parse::<Index>()
        .unwrap_err();
    assert_eq!(error.0, 2);
}

#[test]
fn test_search_terms() {
    let index: Index = SAMPLE_INDEX.parse().unwrap();

    let names = |query| -> Vec<String> {
        index
            .search(query)
            .map(|module| module.name.clone())
            .collect()
    };

    // Matching is case-insensitive, in both directions:
    assert_eq!(names("imap"), ["imap"]);
    assert_eq!(names("IMAP"), ["imap"]);
    assert_eq!(names("llm INFERENCE"), ["anthropic"]);

    // Every term must match, but different terms may match different fields:
    assert_eq!(names("email import"), ["imap", "maildir"]);
    assert_eq!(names("email ipfs"), [] as [&str; 0]);

    // Terms match links, provided programs, and handled inputs:
    assert_eq!(
        names("github.com/asimov-modules"),
        ["anthropic", "imap", "ipfs", "maildir"],
    );
    assert_eq!(names("imap-lister"), ["imap"]);
    assert_eq!(names(".maildir"), ["maildir"]);

    // A term is never matched across a field boundary (name + label here):
    assert_eq!(names("imapimap"), [] as [&str; 0]);

    // Non-matching queries yield nothing:
    assert_eq!(names("zzzznothing"), [] as [&str; 0]);

    // An empty query matches everything, in index order:
    assert_eq!(names(""), ["anthropic", "imap", "ipfs", "maildir"]);
}
