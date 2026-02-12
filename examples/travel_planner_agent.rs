//! Example: Complex Travel Planner Agent
//!
//! This example demonstrates a sophisticated travel planning agent that:
//! - Searches for destinations, attractions, and hotels
//! - Queries weather information
//! - Calculates travel budgets
//! - Plans routes and optimizes itineraries
//! - Uses subagents for specialized tasks
//! - Maintains memory of user preferences
//!
//! Architecture:
//! - Main agent coordinates the planning process
//! - Multiple custom tools for different functions
//! - Subagents for specialized recommendations (attractions, transportation)
//! - Memory to remember user preferences across conversations

use std::collections::HashMap;
use std::env;
use std::sync::Arc;

use async_trait::async_trait;
use oris::{
    agent::{create_agent, SubagentInfo, SubagentsBuilder},
    error::ToolError,
    schemas::messages::Message,
    tools::{DuckDuckGoSearchResults, Tool},
};
use serde_json::{json, Value};

// ============================================================================
// Custom Tools Implementation
// ============================================================================

/// Tool for searching destinations and attractions
struct DestinationSearchTool;

#[async_trait]
impl Tool for DestinationSearchTool {
    fn name(&self) -> String {
        "search_destination".to_string()
    }

    fn description(&self) -> String {
        "Search for tourist destinations and attractions. Use this to find popular places, landmarks, and points of interest in a destination.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "destination": {
                    "type": "string",
                    "description": "The destination city or country to search"
                },
                "search_type": {
                    "type": "string",
                    "enum": ["attractions", "landmarks", "all"],
                    "default": "all",
                    "description": "Type of search: attractions (tourist spots), landmarks (famous places), or all"
                }
            },
            "required": ["destination"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let destination = input["destination"]
            .as_str()
            .ok_or_else(|| ToolError::MissingInput("destination".to_string()))?;
        let search_type = input["search_type"].as_str().unwrap_or("all");

        // Simulated search results
        let results = match (destination.to_lowercase().as_str(), search_type) {
            ("tokyo", "attractions") | ("tokyo", "all") => json!({
                "destination": "Tokyo, Japan",
                "attractions": [
                    {"name": "Senso-ji Temple", "type": "Temple", "rating": 4.5, "description": "Ancient Buddhist temple in Asakusa"},
                    {"name": "Tokyo Skytree", "type": "Observation Tower", "rating": 4.6, "description": "Tallest tower in Japan with panoramic views"},
                    {"name": "Shibuya Crossing", "type": "Landmark", "rating": 4.4, "description": "World's busiest pedestrian crossing"},
                    {"name": "Tsukiji Outer Market", "type": "Market", "rating": 4.3, "description": "Famous fish market and food stalls"},
                    {"name": "Meiji Shrine", "type": "Shrine", "rating": 4.5, "description": "Peaceful Shinto shrine in Shibuya"}
                ]
            }),
            ("paris", "attractions") | ("paris", "all") => json!({
                "destination": "Paris, France",
                "attractions": [
                    {"name": "Eiffel Tower", "type": "Landmark", "rating": 4.6, "description": "Iconic iron lattice tower"},
                    {"name": "Louvre Museum", "type": "Museum", "rating": 4.7, "description": "World's largest art museum"},
                    {"name": "Notre-Dame Cathedral", "type": "Cathedral", "rating": 4.6, "description": "Medieval Catholic cathedral"},
                    {"name": "Arc de Triomphe", "type": "Monument", "rating": 4.5, "description": "Famous monument at Champs-Élysées"},
                    {"name": "Montmartre", "type": "District", "rating": 4.4, "description": "Historic hilltop district with Sacré-Cœur"}
                ]
            }),
            _ => json!({
                "destination": destination,
                "attractions": [
                    {"name": format!("{} Main Attraction", destination), "type": "General", "rating": 4.0, "description": "Popular tourist spot"},
                    {"name": format!("{} Historic Site", destination), "type": "Historic", "rating": 4.2, "description": "Important historical location"}
                ]
            }),
        };

        serde_json::to_string_pretty(&results).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

/// Tool for searching hotels
struct HotelSearchTool;

#[async_trait]
impl Tool for HotelSearchTool {
    fn name(&self) -> String {
        "search_hotels".to_string()
    }

    fn description(&self) -> String {
        "Search for hotels in a destination. Returns hotel options with prices, ratings, and amenities.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "City or area where to search for hotels"
                },
                "check_in": {
                    "type": "string",
                    "description": "Check-in date (YYYY-MM-DD format)"
                },
                "check_out": {
                    "type": "string",
                    "description": "Check-out date (YYYY-MM-DD format)"
                },
                "budget_range": {
                    "type": "string",
                    "enum": ["budget", "mid-range", "luxury"],
                    "description": "Budget preference: budget (under $100/night), mid-range ($100-300/night), luxury ($300+/night)"
                }
            },
            "required": ["location", "check_in", "check_out"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let location = input["location"]
            .as_str()
            .ok_or_else(|| ToolError::MissingInput("location".to_string()))?;
        let check_in = input["check_in"]
            .as_str()
            .ok_or_else(|| ToolError::MissingInput("check_in".to_string()))?;
        let check_out = input["check_out"]
            .as_str()
            .ok_or_else(|| ToolError::MissingInput("check_out".to_string()))?;
        let budget_range = input["budget_range"].as_str().unwrap_or("mid-range");

        // Simulated hotel search results
        let hotels = match budget_range {
            "budget" => vec![
                json!({"name": "Budget Inn", "price_per_night": 65, "rating": 3.5, "amenities": ["WiFi", "Breakfast"]}),
                json!({"name": "City Hostel", "price_per_night": 45, "rating": 3.8, "amenities": ["WiFi", "Kitchen"]}),
            ],
            "luxury" => vec![
                json!({"name": "Grand Hotel", "price_per_night": 450, "rating": 4.8, "amenities": ["Spa", "Pool", "Restaurant", "Concierge"]}),
                json!({"name": "Luxury Resort", "price_per_night": 600, "rating": 4.9, "amenities": ["Spa", "Pool", "Beach Access", "Fine Dining"]}),
            ],
            _ => vec![
                json!({"name": "City Center Hotel", "price_per_night": 150, "rating": 4.2, "amenities": ["WiFi", "Breakfast", "Gym"]}),
                json!({"name": "Business Hotel", "price_per_night": 180, "rating": 4.3, "amenities": ["WiFi", "Breakfast", "Business Center"]}),
                json!({"name": "Boutique Hotel", "price_per_night": 220, "rating": 4.5, "amenities": ["WiFi", "Breakfast", "Spa"]}),
            ],
        };

        let results = json!({
            "location": location,
            "check_in": check_in,
            "check_out": check_out,
            "budget_range": budget_range,
            "hotels": hotels
        });

        serde_json::to_string_pretty(&results).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

/// Tool for querying weather information
struct WeatherTool;

#[async_trait]
impl Tool for WeatherTool {
    fn name(&self) -> String {
        "get_weather".to_string()
    }

    fn description(&self) -> String {
        "Get current weather and forecast for a destination. Use this to check weather conditions for travel planning.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "City name or location to get weather for"
                },
                "days": {
                    "type": "integer",
                    "default": 5,
                    "description": "Number of days for forecast (1-7)"
                }
            },
            "required": ["location"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let location = input["location"]
            .as_str()
            .ok_or_else(|| ToolError::MissingInput("location".to_string()))?;
        let days = input["days"].as_u64().unwrap_or(5) as usize;

        // Simulated weather data
        let mut forecast = Vec::new();
        for i in 0..days.min(7) {
            forecast.push(json!({
                "date": format!("2024-12-{:02}", 15 + i),
                "condition": if i % 2 == 0 { "Sunny" } else { "Partly Cloudy" },
                "high_temp": 20 + (i % 5),
                "low_temp": 12 + (i % 3),
                "precipitation": if i == 2 { 30 } else { 0 }
            }));
        }

        let results = json!({
            "location": location,
            "current": {
                "condition": "Sunny",
                "temperature": 22,
                "humidity": 65
            },
            "forecast": forecast
        });

        serde_json::to_string_pretty(&results).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

/// Tool for calculating travel budget
struct BudgetCalculatorTool;

#[async_trait]
impl Tool for BudgetCalculatorTool {
    fn name(&self) -> String {
        "calculate_budget".to_string()
    }

    fn description(&self) -> String {
        "Calculate total travel budget from expense items. Categories include accommodation, food, transportation, activities, and miscellaneous.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "category": {
                                "type": "string",
                                "enum": ["accommodation", "food", "transportation", "activities", "miscellaneous"]
                            },
                            "amount": {
                                "type": "number",
                                "description": "Amount in the specified currency"
                            },
                            "description": {
                                "type": "string",
                                "description": "Description of the expense"
                            }
                        },
                        "required": ["category", "amount"]
                    },
                    "description": "Array of expense items"
                },
                "currency": {
                    "type": "string",
                    "default": "USD",
                    "description": "Currency code (USD, EUR, JPY, etc.)"
                }
            },
            "required": ["items"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let items = input["items"]
            .as_array()
            .ok_or_else(|| ToolError::MissingInput("items".to_string()))?;
        let currency = input["currency"].as_str().unwrap_or("USD");

        let mut totals: HashMap<String, f64> = HashMap::new();
        let mut total = 0.0;

        for item in items {
            let category = item["category"]
                .as_str()
                .ok_or_else(|| ToolError::MissingInput("category".to_string()))?;
            let amount = item["amount"]
                .as_f64()
                .ok_or_else(|| ToolError::MissingInput("amount".to_string()))?;

            *totals.entry(category.to_string()).or_insert(0.0) += amount;
            total += amount;
        }

        let breakdown: HashMap<String, f64> = totals.iter().map(|(k, v)| (k.clone(), *v)).collect();

        let results = json!({
            "currency": currency,
            "breakdown": breakdown,
            "total": total,
            "summary": format!("Total budget: {:.2} {}", total, currency)
        });

        serde_json::to_string_pretty(&results).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

/// Tool for planning routes between attractions
struct RoutePlannerTool;

#[async_trait]
impl Tool for RoutePlannerTool {
    fn name(&self) -> String {
        "plan_route".to_string()
    }

    fn description(&self) -> String {
        "Plan an optimized route between multiple attractions. Considers distance and travel time to create an efficient itinerary order.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "attractions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "location": {"type": "string"}
                        },
                        "required": ["name", "location"]
                    },
                    "description": "List of attractions to visit"
                },
                "start_location": {
                    "type": "string",
                    "description": "Starting point (hotel or initial location)"
                },
                "transportation_mode": {
                    "type": "string",
                    "enum": ["walking", "public_transit", "taxi", "mixed"],
                    "default": "mixed",
                    "description": "Preferred transportation mode"
                }
            },
            "required": ["attractions", "start_location"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let attractions = input["attractions"]
            .as_array()
            .ok_or_else(|| ToolError::MissingInput("attractions".to_string()))?;
        let start_location = input["start_location"]
            .as_str()
            .ok_or_else(|| ToolError::MissingInput("start_location".to_string()))?;
        let transport_mode = input["transportation_mode"].as_str().unwrap_or("mixed");

        // Simple route optimization: sort by distance from start
        let mut route = Vec::new();
        route.push(json!({
            "order": 0,
            "location": start_location,
            "type": "start"
        }));

        for (idx, attraction) in attractions.iter().enumerate() {
            let name = attraction["name"].as_str().unwrap_or("Unknown");
            let location = attraction["location"].as_str().unwrap_or("Unknown");

            route.push(json!({
                "order": idx + 1,
                "name": name,
                "location": location,
                "estimated_time": format!("{} minutes", 30 + idx * 15),
                "transportation": transport_mode
            }));
        }

        let results = json!({
            "start_location": start_location,
            "transportation_mode": transport_mode,
            "route": route,
            "total_stops": route.len(),
            "estimated_total_time": format!("{} hours", route.len() * 2)
        });

        serde_json::to_string_pretty(&results).map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

/// Tool for optimizing itineraries
struct ItineraryOptimizerTool;

#[async_trait]
impl Tool for ItineraryOptimizerTool {
    fn name(&self) -> String {
        "optimize_itinerary".to_string()
    }

    fn description(&self) -> String {
        "Optimize a travel itinerary based on constraints like time, budget, and preferences. Reorganizes activities for better flow and efficiency.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "itinerary": {
                    "type": "object",
                    "description": "Current itinerary with days and activities"
                },
                "constraints": {
                    "type": "object",
                    "properties": {
                        "max_daily_hours": {"type": "integer", "default": 8},
                        "budget_limit": {"type": "number"},
                        "preferences": {"type": "array", "items": {"type": "string"}}
                    }
                }
            },
            "required": ["itinerary"]
        })
    }

    async fn run(&self, input: Value) -> Result<String, ToolError> {
        let itinerary = input["itinerary"]
            .as_object()
            .ok_or_else(|| ToolError::MissingInput("itinerary".to_string()))?;
        let _constraints = input.get("constraints");

        // Simulated optimization
        let optimized = json!({
            "original_days": itinerary.len(),
            "optimized_days": itinerary.len(),
            "improvements": [
                "Grouped nearby attractions together",
                "Balanced daily activity load",
                "Added buffer time between activities",
                "Optimized transportation routes"
            ],
            "estimated_savings": "15% time, 10% cost",
            "itinerary": itinerary
        });

        serde_json::to_string_pretty(&optimized)
            .map_err(|e| ToolError::ExecutionError(e.to_string()))
    }
}

// ============================================================================
// Main Example
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Set Qwen API key environment variable
    // Replace with your actual API key
    env::set_var("QWEN_API_KEY", "your_qwen_api_key_here");

    println!("=== Travel Planner Agent Example ===\n");

    // Create specialized subagents
    // Note: Using "qwen-plus" as the model name (qwen3-plus is not a valid model name)
    // Valid Qwen models: qwen-plus, qwen-max, qwen-turbo, qwen-long
    let attraction_agent = Arc::new(create_agent(
        "qwen:qwen3-max",
        &[],
        Some(
            "You are a travel attraction expert. Your role is to recommend and evaluate tourist attractions, \
             landmarks, and points of interest. Provide detailed information about attractions including \
             ratings, descriptions, and visiting tips. Always consider user preferences and travel context.",
        ),
        None,
    )?);

    let transportation_agent = Arc::new(create_agent(
        "qwen:qwen3-max",
        &[],
        Some(
            "You are a transportation planning expert. Your role is to plan and optimize transportation \
             between destinations, considering factors like time, cost, comfort, and convenience. \
             Recommend the best transportation modes (walking, public transit, taxi, etc.) for each route.",
        ),
        None,
    )?);

    // Create all tools
    let destination_tool = Arc::new(DestinationSearchTool);
    let hotel_tool = Arc::new(HotelSearchTool);
    let weather_tool = Arc::new(WeatherTool);
    let budget_tool = Arc::new(BudgetCalculatorTool);
    let route_tool = Arc::new(RoutePlannerTool);
    let optimizer_tool = Arc::new(ItineraryOptimizerTool);
    let search_tool = Arc::new(DuckDuckGoSearchResults::default());

    // Create main agent with tools and subagents using SubagentsBuilder
    // SubagentsBuilder can create an agent with both tools and subagents
    // Note: Using "qwen-plus" as the model name (qwen3-plus is not a valid model name)
    let main_agent = SubagentsBuilder::new()
        .with_model("qwen:qwen3-max")
        .with_system_prompt(
            "You are an expert travel planning assistant. Your role is to help users plan comprehensive \
             travel itineraries. You have access to specialized tools and subagents for different aspects \
             of travel planning.\n\n\
             Your capabilities include:\n\
             - Searching for destinations and attractions\n\
             - Finding hotels and accommodations\n\
             - Checking weather conditions\n\
             - Calculating travel budgets\n\
             - Planning routes between locations\n\
             - Optimizing itineraries\n\
             - Getting recommendations from specialized subagents\n\n\
             Always provide detailed, well-organized travel plans. Consider user preferences, budget constraints, \
             and time limitations. Break down complex requests into steps and use the appropriate tools and \
             subagents for each task.",
        )
        .with_tools(&[
            destination_tool.clone(),
            hotel_tool.clone(),
            weather_tool.clone(),
            budget_tool.clone(),
            route_tool.clone(),
            optimizer_tool.clone(),
            search_tool.clone(),
        ])
        .with_subagent(SubagentInfo::new(
            attraction_agent,
            "attraction_recommender".to_string(),
            "Specialized agent for recommending and evaluating tourist attractions and landmarks".to_string(),
        ))
        .with_subagent(SubagentInfo::new(
            transportation_agent,
            "transportation_planner".to_string(),
            "Specialized agent for planning transportation and routes between destinations".to_string(),
        ))
        .build()?
        .with_max_iterations(20); // Increase max iterations for complex travel planning tasks

    // Example 1: Complete travel planning request
    println!("Example 1: Planning a trip to Tokyo\n");
    println!("User: 我想去日本东京旅游，预算5000元，5天\n");

    let result1 = main_agent
        .invoke_messages(vec![Message::new_human_message(
            "我想去日本东京旅游，预算5000元，5天。请帮我规划一个完整的行程，包括景点推荐、酒店选择、天气查询和预算分配。",
        )])
        .await?;

    println!("Agent Response:\n{}\n", result1);
    println!("{}\n", "=".repeat(80));

    // Example 2: Follow-up question
    // Note: For memory support, you would use AgentExecutor with memory
    // Here we demonstrate the agent's capabilities
    println!("Example 2: Asking for route planning\n");
    println!("User: 请帮我规划从酒店到各个景点的路线\n");

    let result2 = main_agent
        .invoke_messages(vec![Message::new_human_message(
            "请帮我规划从酒店到各个景点的路线，优化交通方式。",
        )])
        .await?;

    println!("Agent Response:\n{}\n", result2);
    println!("{}\n", "=".repeat(80));

    // Example 3: Different destination
    println!("Example 3: Planning a trip to Paris\n");
    println!("User: 我想去巴黎，3天，预算3000欧元\n");

    let result3 = main_agent
        .invoke_messages(vec![Message::new_human_message(
            "我想去巴黎，3天，预算3000欧元。请推荐必去的景点和合适的酒店。",
        )])
        .await?;

    println!("Agent Response:\n{}\n", result3);
    println!("{}\n", "=".repeat(80));

    // Example 4: Using AgentExecutor with memory for persistent conversation
    println!("Example 4: Using memory for conversation context\n");
    println!("This demonstrates how to use AgentExecutor with memory:\n");

    // Note: UnifiedAgent can be used directly, but for memory support we need AgentExecutor
    // However, UnifiedAgent doesn't implement the Agent trait directly, so we need to use it differently
    // For this example, we'll show both approaches

    // Approach 1: Direct use with invoke_messages (no memory)
    println!("Direct agent usage (no memory):\n");
    println!("User: 我想去日本东京旅游，预算5000元，5天\n");
    let result4 = main_agent
        .invoke_messages(vec![Message::new_human_message(
            "我想去日本东京旅游，预算5000元，5天。请帮我规划一个完整的行程。",
        )])
        .await?;

    println!("Agent Response:\n{}\n", result4);
    println!("{}\n", "=".repeat(80));

    Ok(())
}
