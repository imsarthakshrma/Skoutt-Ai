// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — intelligence/chat.rs
// Interactive REPL to discuss status, plans, and instructions with Scott.
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use tracing::{info, warn};

use crate::database::Database;

pub struct ChatSession {
    api_key: String,
    model: String,
    client: Client,
    db: Database,
    history: Vec<ChatMessage>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ChatMessage>,
    system: String,
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
}

#[derive(Deserialize)]
struct ClaudeContent {
    text: String,
}

impl ChatSession {
    pub fn new(api_key: String, model: String, db: Database) -> Self {
        Self {
            api_key,
            model,
            client: Client::new(),
            db,
            history: Vec::new(),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        let mut rl = DefaultEditor::new()?;
        
        println!("\n{}", "━".repeat(60));
        println!("  SKOUTT INTERACTIVE SESSION — Chatting with Scott (Dubai)");
        println!("  Type 'exit' or 'quit' to end session.");
        println!("{}", "━".repeat(60));

        loop {
            let readline = rl.readline(">> ");
            match readline {
                Ok(line) => {
                    let input = line.trim();
                    if input.is_empty() { continue; }
                    if input == "exit" || input == "quit" { break; }
                    
                    rl.add_history_entry(input)?;
                    
                    self.history.push(ChatMessage {
                        role: "user".into(),
                        content: input.into(),
                    });

                    print!("Thinking...");
                    std::io::Write::flush(&mut std::io::stdout())?;
                    
                    match self.get_claude_response().await {
                        Ok(response) => {
                            // Clear "Thinking..."
                            print!("\r           \r");
                            println!("\nScott: {}", response);
                            self.history.push(ChatMessage {
                                role: "assistant".into(),
                                content: response,
                            });
                        }
                        Err(e) => {
                            print!("\r           \r");
                            println!("❌ Error: {}", e);
                        }
                    }
                }
                Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => break,
                Err(err) => {
                    println!("❌ Error: {:?}", err);
                    break;
                }
            }
        }

        println!("Goodbye!");
        Ok(())
    }

    async fn get_claude_response(&self) -> Result<String> {
        let stats = self.get_db_stats().await?;
        
        let system_prompt = format!(
            r#"You are Scott, an exhibition expert based in Dubai at Track Exhibits Pvt LTD. 
You are having an interactive conversation with your user about Skoutt (your autonomous lead generation agent) and your exhibition plans.

CURRENT CONTEXT:
Dubai/GST Time: {}
Global Operations: You serve clients worldwide for booth design, fabrication, and installation.

DATABASE STATUS:
{}

YOUR ROLE:
- Discuss progress, plans, and instructions for Skoutt.
- Be professional, knowledgeable, and helpful as an exhibition veteran.
- If the user gives new directions (e.g. "Focus on Tech in Europe"), acknowledge them and explain how it helps the strategy.
- You have a global perspective but are rooted in Dubai's strategic hub."#,
            chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(4 * 3600).unwrap()).format("%Y-%m-%d %H:%M"),
            stats
        );

        let request = ClaudeRequest {
            model: self.model.clone(),
            max_tokens: 1000,
            system: system_prompt,
            messages: self.history.clone(),
        };

        let response = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Claude API error: {}", response.status()));
        }

        let body: ClaudeResponse = response.json().await?;
        Ok(body.content.into_iter().next().map(|c| c.text).unwrap_or_default())
    }

    async fn get_db_stats(&self) -> Result<String> {
        let exhibitions = sqlx::query!("SELECT COUNT(*) as count FROM exhibitions").fetch_one(&self.db.pool).await?.count;
        let companies = sqlx::query!("SELECT COUNT(*) as count FROM companies").fetch_one(&self.db.pool).await?.count;
        let contacts = sqlx::query!("SELECT COUNT(*) as count FROM contacts").fetch_one(&self.db.pool).await?.count;
        let interested = sqlx::query!("SELECT COUNT(*) as count FROM emails_sent WHERE interest_level IN ('High', 'Medium')").fetch_one(&self.db.pool).await?.count;

        Ok(format!(
            "- Exhibitions: {}\n- Companies: {}\n- Contacts: {}\n- Interested leads: {}",
            exhibitions, companies, contacts, interested
        ))
    }
}
