use super::*;

#[tokio::test]
async fn save_config_performs_atomic_write() {
    let temp_dir = std::env::temp_dir().join(format!(
        "miao-test-save-{}-{}",
        std::process::id(),
        "atomic"
    ));
    let config_path = temp_dir.join("nested").join("config.yaml");

    let config = Config {
        port: Some(8080),
        subs: vec!["https://example.com/sub".to_string()],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    };

    save_config_to(&config_path, &config).await.unwrap();

    let content = tokio::fs::read_to_string(&config_path).await.unwrap();
    let parsed: Config = yaml_serde::from_str(&content).unwrap();
    assert_eq!(parsed.port, Some(8080));
    assert_eq!(parsed.subs.len(), 1);

    // 清理
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
}

#[tokio::test]
async fn save_config_overwrites_existing_file() {
    let temp_dir = std::env::temp_dir().join(format!(
        "miao-test-save-{}-{}",
        std::process::id(),
        "overwrite"
    ));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    let config_path = temp_dir.join("config.yaml");

    // 先创建旧配置
    tokio::fs::write(
        &config_path,
        "port: 9999\nsubs: []\nnodes: []\ncustom_rules: []",
    )
    .await
    .unwrap();

    // 使用原子写入保存新配置
    let config = Config {
        port: Some(7777),
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    };
    save_config_to(&config_path, &config).await.unwrap();

    let content = tokio::fs::read_to_string(&config_path).await.unwrap();
    let parsed: Config = yaml_serde::from_str(&content).unwrap();
    assert_eq!(parsed.port, Some(7777));

    // 清理
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
}

#[tokio::test]
async fn save_config_skips_identical_content() {
    let temp_dir =
        std::env::temp_dir().join(format!("miao-test-save-{}-{}", std::process::id(), "skip"));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    let config_path = temp_dir.join("config.yaml");
    let config = Config {
        port: Some(6161),
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    };

    save_config_to(&config_path, &config).await.unwrap();
    let before = tokio::fs::metadata(&config_path)
        .await
        .unwrap()
        .modified()
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    save_config_to(&config_path, &config).await.unwrap();

    let after = tokio::fs::metadata(&config_path)
        .await
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
}
