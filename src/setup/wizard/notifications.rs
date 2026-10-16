//! Issue #1454: the optional Telegram/Discord step of `gah setup`. Off is the
//! default and asks for nothing; choosing a channel takes the operator to a
//! working channel in one pass: credential (hidden), chat id, test message.

use super::Setup;
use crate::notify_channels::NotificationChannel;
use anyhow::{bail, Result};

/// The module as `gah setup` sees and saves it. Never holds the credential.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub channel: NotificationChannel,
    pub telegram_chat_id: Option<String>,
    /// A credential is already in the private credential file.
    pub credential_saved: bool,
}

const CHANNELS: [NotificationChannel; 3] = [
    NotificationChannel::None,
    NotificationChannel::Telegram,
    NotificationChannel::Discord,
];

impl Setup<'_> {
    /// Runs the step. A problem is reported and never fails setup.
    pub fn notifications(&mut self) {
        if let Err(error) = self.notifications_step() {
            self.prompter
                .say(&format!("! Notifications were not changed: {error:#}"));
            self.prompter.say(
                "  Try again with `gah setup --notifications-only`, or in Settings → Notifications.",
            );
        }
    }

    fn notifications_step(&mut self) -> Result<()> {
        let current = self.effects.notification_settings();
        match (self.options.notifications_enabled, self.options.yes) {
            (Some(false), _) => {
                if current.enabled {
                    self.save_notifications(
                        NotificationSettings {
                            enabled: false,
                            ..current
                        },
                        None,
                    )?;
                }
                self.prompter.say("Notifications are off.");
                return Ok(());
            }
            // Unattended: switch on what an earlier setup or Settings saved.
            (Some(true), true) => {
                if current.channel == NotificationChannel::None {
                    bail!("no channel is saved yet; run `gah setup --notifications-only` in a terminal");
                }
                self.save_notifications(
                    NotificationSettings {
                        enabled: true,
                        ..current
                    },
                    None,
                )?;
                self.test_notification();
                return Ok(());
            }
            (None, true) => return Ok(()),
            _ => {}
        }
        self.prompter.say("");
        self.prompter.say("Notifications (optional). Parasaur can message you on Telegram or Discord when work fails, a review is ready, or a node goes offline. Change this any time in Settings → Notifications.");
        let default = if current.enabled {
            CHANNELS
                .iter()
                .position(|c| *c == current.channel)
                .unwrap_or(0)
        } else {
            0
        };
        let options = [
            "Off".to_string(),
            "Telegram".to_string(),
            "Discord".to_string(),
        ];
        let channel = CHANNELS[self.prompter.choose(
            "Send notifications to a chat app?",
            &options,
            default,
        )?];
        if channel == NotificationChannel::None {
            if current.enabled {
                self.save_notifications(
                    NotificationSettings {
                        enabled: false,
                        ..current
                    },
                    None,
                )?;
            }
            self.prompter.say("Notifications are off.");
            return Ok(());
        }
        let keep = current.channel == channel && current.credential_saved;
        let (credential, telegram_chat_id) = if channel == NotificationChannel::Telegram {
            self.prompter.say("  1. In Telegram, message @BotFather, send /newbot, and copy the token it replies with (it looks like 123456789:AAE...).");
            self.prompter
                .say("  2. Open a chat with your new bot and press Start, so it may message you.");
            let token = self.ask_credential("Bot token (hidden)", keep)?;
            self.prompter.say("  3. Your chat id is a number: message @userinfobot and it replies with yours. For a group, add the bot to it; a group id starts with -100.");
            let chat_id = self
                .prompter
                .text("Chat id", current.telegram_chat_id.as_deref())?;
            let chat_id = chat_id.trim();
            if chat_id.is_empty() {
                bail!("a Telegram chat id is required");
            }
            (token, Some(chat_id.to_string()))
        } else {
            self.prompter.say("  In Discord, open Server Settings → Integrations → Webhooks → New Webhook, choose the channel, and select Copy Webhook URL.");
            let url = self.ask_credential("Webhook URL (hidden)", keep)?;
            (url, current.telegram_chat_id.clone())
        };
        self.save_notifications(
            NotificationSettings {
                enabled: true,
                channel,
                telegram_chat_id,
                credential_saved: true,
            },
            credential.as_deref(),
        )?;
        self.prompter.say(&format!(
            "✓ Notifications are on ({}). The credential is in ~/.config/gah/notify.env (private).",
            channel.as_str()
        ));
        self.test_notification();
        Ok(())
    }

    fn save_notifications(
        &mut self,
        settings: NotificationSettings,
        credential: Option<&str>,
    ) -> Result<()> {
        self.effects
            .save_notification_settings(&settings, credential)
    }

    /// `None` keeps the saved credential (only offered when one exists).
    fn ask_credential(&mut self, question: &str, keep: bool) -> Result<Option<String>> {
        let question = if keep {
            format!("{question}, empty keeps the saved one")
        } else {
            question.to_string()
        };
        let value = self.prompter.secret(&question)?;
        match value.trim() {
            "" if keep => Ok(None),
            "" => bail!("no credential was entered"),
            value => Ok(Some(value.to_string())),
        }
    }

    /// A failed test leaves the module on: the operator asked for it, and
    /// the failure is what they need to see to repair it.
    fn test_notification(&mut self) {
        match self.effects.send_test_notification() {
            Ok(()) => self
                .prompter
                .say("✓ Sent a test message. Check that it arrived."),
            Err(error) => self.prompter.say(&format!(
                "! The test message failed: {error:#}. Notifications stay on; fix it with `gah setup --notifications-only` or send another test from Settings → Notifications."
            )),
        }
    }
}
