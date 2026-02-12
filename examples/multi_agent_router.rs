use std::collections::HashMap;
use std::sync::Arc;

use oris::{
    agent::{create_agent, RouterAgentBuilder},
    llm::openai::{OpenAI, OpenAIModel},
    schemas::messages::Message,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    // Create specialized agents
    let weather_agent = Arc::new(create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a weather information agent. Provide weather-related information."),
        None,
    )?);

    let news_agent = Arc::new(create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a news agent. Provide news and current events information."),
        None,
    )?);

    let default_agent = Arc::new(create_agent(
        "gpt-4o-mini",
        &[],
        Some("You are a general assistant."),
        None,
    )?);

    // Create LLM for routing
    let routing_llm: Box<dyn oris::language_models::llm::LLM> =
        Box::new(OpenAI::default().with_model(OpenAIModel::Gpt4oMini.to_string()));

    // Create router agent with LLM-based routing
    let router_agent = RouterAgentBuilder::new()
        .with_llm_router(
            routing_llm,
            vec![
                (
                    "weather_agent".to_string(),
                    "Handles weather-related queries".to_string(),
                ),
                (
                    "news_agent".to_string(),
                    "Handles news and current events queries".to_string(),
                ),
            ],
        )
        .with_agent("weather_agent".to_string(), weather_agent)
        .with_agent("news_agent".to_string(), news_agent)
        .with_default_agent(default_agent)
        .build()?;

    println!("Testing Router pattern with LLM-based routing...\n");

    // Test 1: Weather query
    println!("Question: What's the weather like today?");
    let response = router_agent
        .invoke_messages(vec![Message::new_human_message(
            "What's the weather like today?",
        )])
        .await?;
    println!("Response: {}\n", response);

    // Test 2: News query
    println!("Question: What are the latest tech news?");
    let response = router_agent
        .invoke_messages(vec![Message::new_human_message(
            "What are the latest tech news?",
        )])
        .await?;
    println!("Response: {}\n", response);

    // Test 3: Keyword-based routing
    let mut keyword_map = HashMap::new();
    keyword_map.insert(
        "weather_agent".to_string(),
        vec![
            "weather".to_string(),
            "temperature".to_string(),
            "forecast".to_string(),
        ],
    );
    keyword_map.insert(
        "news_agent".to_string(),
        vec![
            "news".to_string(),
            "headlines".to_string(),
            "current events".to_string(),
        ],
    );

    let keyword_router_agent = RouterAgentBuilder::new()
        .with_keyword_router(keyword_map)
        .with_agent(
            "weather_agent".to_string(),
            Arc::new(create_agent(
                "gpt-4o-mini",
                &[],
                Some("Weather agent"),
                None,
            )?),
        )
        .with_agent(
            "news_agent".to_string(),
            Arc::new(create_agent("gpt-4o-mini", &[], Some("News agent"), None)?),
        )
        .build()?;

    println!("Testing Router pattern with keyword-based routing...\n");
    println!("Question: What's the forecast for tomorrow?");
    let response = keyword_router_agent
        .invoke_messages(vec![Message::new_human_message(
            "What's the forecast for tomorrow?",
        )])
        .await?;
    println!("Response: {}\n", response);

    Ok(())
}
