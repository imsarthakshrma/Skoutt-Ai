// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — outreach/reply_monitor.rs
// IMAP inbox monitoring for incoming replies (tokio-rustls, no OpenSSL)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use async_imap::Client;
use futures::StreamExt;
use rustls_native_certs::load_native_certs;
use std::sync::Arc;
use tokio_rustls::{rustls, TlsConnector};
use tracing::{info, warn};

use crate::{database::Database, intelligence::reply_analyzer::IncomingReply, ImapConfig};

pub struct ReplyMonitor {
    config: ImapConfig,
}

impl ReplyMonitor {
    pub fn new(config: ImapConfig) -> Self {
        Self { config }
    }

    /// Check inbox for new replies and return matched replies
    pub async fn check_inbox(&self, db: &Database) -> Result<Vec<IncomingReply>> {
        // Build TLS connector using system root certs (pure Rust, no OpenSSL)
        let mut root_store = rustls::RootCertStore::empty();
        for cert in load_native_certs().unwrap_or_default() {
            let _ = root_store.add(cert);
        }
        let tls_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(tls_config));

        let tcp_stream = tokio::net::TcpStream::connect(
            format!("{}:{}", self.config.host, self.config.port)
        ).await?;

        let server_name = rustls::pki_types::ServerName::try_from(self.config.host.clone())
            .map_err(|e| anyhow::anyhow!("Invalid IMAP hostname: {e}"))?;
        let tls_stream = connector.connect(server_name, tcp_stream).await?;
        let client = Client::new(tls_stream);

        let mut imap_session = client
            .login(&self.config.username, &self.config.password)
            .await
            .map_err(|(e, _)| anyhow::anyhow!("IMAP login failed: {}", e))?;

        imap_session.select(&self.config.mailbox).await?;

        // Search for unseen messages from the last 24 hours
        let since_date = (chrono::Utc::now() - chrono::Duration::days(1))
            .format("%d-%b-%Y")
            .to_string();

        let message_ids = imap_session
            .search(format!("UNSEEN SINCE {}", since_date))
            .await?;

        info!("  IMAP: {} unseen messages to check", message_ids.len());

        let mut replies = Vec::new();
        let ids: Vec<u32> = message_ids.into_iter().take(50).collect();

        for msg_id in ids {
            match self.fetch_and_parse_message(&mut imap_session, msg_id, db).await {
                Ok(Some(reply)) => replies.push(reply),
                Ok(None) => {}
                Err(e) => warn!("  Failed to parse message {}: {}", msg_id, e),
            }
        }

        imap_session.logout().await?;
        Ok(replies)
    }

    async fn fetch_and_parse_message(
        &self,
        session: &mut async_imap::Session<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>,
        msg_id: u32,
        db: &Database,
    ) -> Result<Option<IncomingReply>> {
        let mut stream = session.fetch(msg_id.to_string(), "RFC822").await?;

        while let Some(result) = stream.next().await {
            let message = result?;
            if let Some(body) = message.body() {
                let parsed = mail_parser::MessageParser::default()
                    .parse(body)
                    .ok_or_else(|| anyhow::anyhow!("Failed to parse email"))?;

                let from_email = parsed
                    .from()
                    .and_then(|f| f.first())
                    .and_then(|addr| addr.address())
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                let from_name = parsed
                    .from()
                    .and_then(|f| f.first())
                    .and_then(|addr| addr.name())
                    .map(|s| s.to_string());

                let subject = parsed.subject().unwrap_or("").to_string();
                let body_text = parsed.body_text(0).map(|b| b.to_string()).unwrap_or_default();

                if body_text.is_empty() || from_email.is_empty() {
                    continue;
                }

                // Only process if it's a reply to one of our emails
                if let Ok(Some(contact)) = db.get_contact_by_email(&from_email).await {
                    if let Ok(Some(email_record)) = db.get_latest_email_for_contact(&contact.id).await {
                        // Handle unsubscribe
                        if subject.to_lowercase().contains("unsubscribe")
                            || body_text.to_lowercase().contains("unsubscribe")
                        {
                            let _ = db.mark_do_not_contact(&from_email).await;
                            info!("  Unsubscribe processed for {}", from_email);
                            continue;
                        }

                        return Ok(Some(IncomingReply {
                            email_id: email_record.id,
                            from_email,
                            from_name,
                            subject,
                            body: body_text,
                            received_at: chrono::Utc::now(),
                        }));
                    }
                }
            }
        }

        Ok(None)
    }
}
