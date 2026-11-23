//! Claude Code runs: the same supervision as [`crate::agent_runs`], over
//! Claude Code's own host protocol instead of ACP.
//!
//! Claude Code is started in print mode with JSON lines both ways and its
//! permission prompts handed to the host (`--permission-prompt-tool stdio`):
//! each step it wants to take arrives as a `can_use_tool` request, which
//! FRIDAY answers by the same rules. A question for the person
//! (`AskUserQuestion`) arrives the same way and is answered like any other
//! question. Claude Code is never tested live here: that would spend the
//! person's plan.

use serde_json::{json, Value};
use tokio::io::AsyncWrite;

use crate::agent_policy::{Kind, Step};
use crate::agent_runs::{Driver, Lines, Outcome, IDLE_TIMEOUT, MAX_REPLIES};

/// The arguments that start Claude Code for a run.
pub const ARGS: [&str; 10] = [
    "-p",
    "--output-format",
    "stream-json",
    "--verbose",
    "--input-format",
    "stream-json",
    "--permission-prompt-tool",
    "stdio",
    "--permission-mode",
    "manual",
];

/// The step a `can_use_tool` request asks about.
pub fn step_from_claude(tool: &str, input: &Value) -> Step {
    let paths = ["file_path", "path", "notebook_path"]
        .iter()
        .filter_map(|k| input[*k].as_str().map(str::to_string))
        .collect();
    Step {
        kind: Kind::from_claude_tool(tool),
        command: input["command"].as_str().map(str::to_string),
        paths,
    }
}

/// A short line for a step: its command, its file, else the tool's name.
fn describe(tool: &str, input: &Value) -> String {
    input["command"]
        .as_str()
        .or_else(|| input["file_path"].as_str())
        .or_else(|| input["url"].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| tool.to_string())
        .chars()
        .take(200)
        .collect()
}

/// An `AskUserQuestion` as one message: each question with its choices.
fn question_text(input: &Value) -> String {
    input["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|q| {
            let choices: Vec<&str> = q["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|o| o["label"].as_str())
                .collect();
            format!(
                "{} (choices: {})",
                q["question"].as_str().unwrap_or(""),
                choices.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn user_message(text: &str) -> Value {
    json!({"type": "user", "message": {"role": "user", "content": text}})
}

impl<W: AsyncWrite + Unpin> Driver<'_, W> {
    async fn respond(&mut self, request_id: &Value, response: Value) -> Result<(), String> {
        self.send(json!({"type": "control_response", "response":
            {"subtype": "success", "request_id": request_id, "response": response}}))
            .await
    }

    /// Answers one `can_use_tool` request.
    async fn can_use_tool(
        &mut self,
        request: &Value,
        replies: &mut usize,
    ) -> Result<Value, String> {
        let tool = request["tool_name"].as_str().unwrap_or("");
        let input = &request["input"];
        if tool == "AskUserQuestion" {
            let asked = question_text(input);
            let by_friday = if *replies < MAX_REPLIES {
                self.reply_to(&asked).await
            } else {
                None
            };
            let answer = match by_friday {
                Some(a) => {
                    *replies += 1;
                    self.rec
                        .log(json!({"event": "answered", "by": "friday", "text": a}));
                    Some(a)
                }
                None => self.ask_person_question(&asked).await,
            };
            let Some(answer) = answer else {
                return Ok(json!({"behavior": "deny",
                    "message": "The person didn't answer. Stop here and say what you need from them."}));
            };
            // The same answer goes to every question asked at once; it is
            // written for all of them.
            let answers: serde_json::Map<String, Value> = input["questions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|q| q["question"].as_str())
                .map(|q| (q.to_string(), json!(answer)))
                .collect();
            return Ok(json!({"behavior": "allow", "updatedInput":
                {"questions": input["questions"], "answers": answers}}));
        }
        let what = describe(tool, input);
        if self
            .decide_step(&what, &step_from_claude(tool, input), true)
            .await
        {
            Ok(json!({"behavior": "allow", "updatedInput": input}))
        } else {
            Ok(
                json!({"behavior": "deny", "message": "Not allowed: the person said no to this step."}),
            )
        }
    }

    /// The whole run over Claude Code's protocol.
    pub async fn drive_claude(&mut self, lines: &mut Lines) -> Result<Outcome, String> {
        self.send(
            json!({"type": "control_request", "request_id": "friday_init",
            "request": {"subtype": "initialize", "hooks": null}}),
        )
        .await?;
        let task = self.rec.run.task.clone();
        self.send(user_message(&task)).await?;
        let mut replies = 0;
        loop {
            let line = tokio::time::timeout(IDLE_TIMEOUT, lines.recv())
                .await
                .map_err(|_| "Claude Code went quiet for 20 minutes".to_string())?
                .ok_or("Claude Code quit")?;
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            // The session to resume for a follow-up (its id can change on resume).
            if let Some(id) = msg["session_id"].as_str() {
                if self.rec.run.session.as_deref() != Some(id) {
                    self.rec.run.session = Some(id.to_string());
                    let _ = self.rec.save();
                }
            }
            match msg["type"].as_str() {
                Some("control_request") if msg["request"]["subtype"] == "can_use_tool" => {
                    let response = self.can_use_tool(&msg["request"], &mut replies).await?;
                    self.respond(&msg["request_id"], response).await?;
                }
                Some("control_request") => {
                    // Nothing else is offered: say so rather than leave it waiting.
                    self.send(
                        json!({"type": "control_response", "response": {"subtype": "error",
                        "request_id": msg["request_id"], "error": "not offered by FRIDAY"}}),
                    )
                    .await?;
                }
                Some("assistant") => {
                    for block in msg["message"]["content"].as_array().into_iter().flatten() {
                        match block["type"].as_str() {
                            Some("text") => {
                                self.said = block["text"].as_str().unwrap_or("").to_string()
                            }
                            Some("tool_use") => {
                                let what =
                                    describe(block["name"].as_str().unwrap_or(""), &block["input"]);
                                self.steps
                                    .insert(block["id"].as_str().unwrap_or("").to_string(), what);
                            }
                            _ => {}
                        }
                    }
                }
                Some("user") => {
                    for block in msg["message"]["content"].as_array().into_iter().flatten() {
                        if block["type"] != "tool_result" {
                            continue;
                        }
                        let step = self
                            .steps
                            .get(block["tool_use_id"].as_str().unwrap_or(""))
                            .cloned()
                            .unwrap_or_else(|| "a step".into());
                        let event = if block["is_error"] == true {
                            "step_failed"
                        } else {
                            "step_done"
                        };
                        self.rec.log(json!({"event": event, "step": step}));
                    }
                }
                Some("result") => {
                    let said = msg["result"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| self.said.clone())
                        .trim()
                        .to_string();
                    self.rec.log(json!({"event": "agent_said", "text": said}));
                    if msg["subtype"] != "success" {
                        let errors: Vec<&str> = msg["errors"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .collect();
                        return Ok(Outcome {
                            summary: format!("{said}\n(stopped: {})", errors.join("; "))
                                .trim()
                                .to_string(),
                            failed: true,
                        });
                    }
                    if !said.ends_with('?') {
                        let summary = match self.refused.take() {
                            Some(step) if said.is_empty() => {
                                format!("Stopped: it wasn't allowed to run {step}")
                            }
                            _ => said,
                        };
                        return Ok(Outcome {
                            summary,
                            failed: false,
                        });
                    }
                    let by_friday = if replies < MAX_REPLIES {
                        self.reply_to(&said).await
                    } else {
                        None
                    };
                    let next = if let Some(r) = by_friday {
                        replies += 1;
                        self.rec
                            .log(json!({"event": "answered", "by": "friday", "text": r}));
                        r
                    } else if let Some(r) = self.ask_person_question(&said).await {
                        r
                    } else {
                        return Ok(Outcome {
                            summary: said,
                            failed: false,
                        });
                    };
                    self.said.clear();
                    self.send(user_message(&next)).await?;
                }
                _ => {}
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::agent_runs::{read_lines, reply, Agent, Recorder, Run, Status};
    use async_trait::async_trait;
    use friday_investigation::LlmStrategyClient;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn recorder(dir: &Path) -> Recorder {
        Recorder::new(
            dir,
            Run {
                id: "rclaude".into(),
                agent: Agent::Claude,
                folder: PathBuf::from("/w/site"),
                request: "add a README; use the MIT license".into(),
                task: "Add a README.".into(),
                status: Status::Running,
                started: 1,
                ended: None,
                pending: None,
                summary: String::new(),
                approved_by_friday: 0,
                approved_by_you: 0,
                rejected: 0,
                session: None,
                follows: None,
                worktree: None,
            },
        )
        .unwrap()
    }

    struct Model(&'static str);

    #[async_trait]
    impl LlmStrategyClient for Model {
        async fn complete(&self, _prompt: &str) -> Result<String, String> {
            Ok(self.0.into())
        }
    }

    fn can_use(id: &str, tool: &str, input: Value) -> String {
        json!({"type": "control_request", "request_id": id,
            "request": {"subtype": "can_use_tool", "tool_name": tool, "input": input}})
        .to_string()
    }

    /// A fake Claude Code: reads FRIDAY's initialize and prompt, sends each
    /// of `asks`, records FRIDAY's control responses, then ends with
    /// `final_words`.
    async fn fake_claude(
        to_friday: tokio::io::DuplexStream,
        from_friday: tokio::io::DuplexStream,
        asks: Vec<String>,
        final_words: &'static str,
    ) -> Vec<Value> {
        let mut out = to_friday;
        let mut lines = BufReader::new(from_friday).lines();
        let mut seen = Vec::new();
        // initialize, then the user's task
        for _ in 0..2 {
            let l = lines.next_line().await.unwrap().unwrap();
            seen.push(serde_json::from_str::<Value>(&l).unwrap());
        }
        for a in &asks {
            out.write_all(format!("{a}\n").as_bytes()).await.unwrap();
            let l = lines.next_line().await.unwrap().unwrap();
            seen.push(serde_json::from_str::<Value>(&l).unwrap());
        }
        let result = json!({"type": "result", "subtype": "success", "result": final_words, "is_error": false});
        out.write_all(format!("{result}\n").as_bytes())
            .await
            .unwrap();
        if final_words.ends_with('?') {
            let l = lines.next_line().await.unwrap().unwrap();
            seen.push(serde_json::from_str::<Value>(&l).unwrap());
            let done = json!({"type": "result", "subtype": "success", "result": "Done.", "is_error": false});
            out.write_all(format!("{done}\n").as_bytes()).await.unwrap();
        }
        seen
    }

    #[tokio::test]
    async fn claude_steps_go_through_the_rules_and_questions_get_answers() {
        let dir =
            TempDir(std::env::temp_dir().join(format!("friday-claude-{}", std::process::id())));
        std::fs::create_dir_all(&dir.0).unwrap();
        let mut rec = recorder(&dir.0);
        let (friday_out, claude_in) = duplex(1 << 16);
        let (claude_out, friday_in) = duplex(1 << 16);
        let question = json!({"questions": [{"question": "Which license?", "header": "License",
            "options": [{"label": "MIT", "description": ""}, {"label": "GPL", "description": ""}],
            "multiSelect": false}]});
        let claude = tokio::spawn(fake_claude(
            claude_out,
            claude_in,
            vec![
                can_use("q1", "Bash", json!({"command": "git status"})),
                can_use(
                    "q2",
                    "Write",
                    json!({"file_path": "/w/site/README.md", "content": "hi"}),
                ),
                can_use("q3", "Bash", json!({"command": "git push"})),
                can_use("q4", "AskUserQuestion", question.clone()),
            ],
            "All done.",
        ));
        // The person refuses the push when it waits.
        let watch = dir.0.clone();
        let person = tokio::spawn(async move {
            for _ in 0..300 {
                let run: Run =
                    serde_json::from_slice(&std::fs::read(watch.join("rclaude.json")).unwrap())
                        .unwrap();
                if run.status == Status::WaitingForYou {
                    crate::agent_runs::answer(&watch, "rclaude", false).unwrap();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("the push never waited");
        });
        let mut lines = read_lines(friday_in);
        let mut d = Driver::new(&mut rec, friday_out, Some(Arc::new(Model("MIT"))));
        d.poll = Duration::from_millis(10);
        let outcome = d.drive_claude(&mut lines).await.unwrap();
        person.await.unwrap();
        let seen = claude.await.unwrap();
        assert_eq!(seen[0]["request"]["subtype"], "initialize");
        assert_eq!(seen[1]["message"]["content"], "Add a README.");
        let response = |i: usize| seen[i]["response"]["response"].clone();
        assert_eq!(seen[2]["response"]["request_id"], "q1");
        assert_eq!(response(2)["behavior"], "allow");
        assert_eq!(response(2)["updatedInput"]["command"], "git status");
        assert_eq!(response(3)["behavior"], "allow");
        assert_eq!(response(4)["behavior"], "deny");
        assert_eq!(response(5)["behavior"], "allow");
        assert_eq!(
            response(5)["updatedInput"]["answers"]["Which license?"],
            "MIT"
        );
        assert_eq!(
            response(5)["updatedInput"]["questions"],
            question["questions"]
        );
        assert_eq!(outcome.summary, "All done.");
        assert_eq!((rec.run.approved_by_friday, rec.run.rejected), (2, 1));
    }

    #[tokio::test]
    async fn a_question_at_the_end_goes_to_the_person_and_their_reply_continues_the_run() {
        let dir =
            TempDir(std::env::temp_dir().join(format!("friday-claude-q-{}", std::process::id())));
        std::fs::create_dir_all(&dir.0).unwrap();
        let mut rec = recorder(&dir.0);
        let (friday_out, claude_in) = duplex(1 << 16);
        let (claude_out, friday_in) = duplex(1 << 16);
        let claude = tokio::spawn(fake_claude(
            claude_out,
            claude_in,
            vec![],
            "Shall I also add a CHANGELOG?",
        ));
        let watch = dir.0.clone();
        let person = tokio::spawn(async move {
            for _ in 0..300 {
                let run: Run =
                    serde_json::from_slice(&std::fs::read(watch.join("rclaude.json")).unwrap())
                        .unwrap();
                if run.status == Status::WaitingForYou {
                    reply(&watch, "rclaude", "No, just the README.").unwrap();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("the question never waited");
        });
        let mut lines = read_lines(friday_in);
        let mut d = Driver::new(&mut rec, friday_out, Some(Arc::new(Model("ASK_PERSON"))));
        d.poll = Duration::from_millis(10);
        let outcome = d.drive_claude(&mut lines).await.unwrap();
        person.await.unwrap();
        let seen = claude.await.unwrap();
        assert_eq!(seen[2]["message"]["content"], "No, just the README.");
        assert_eq!(outcome.summary, "Done.");
    }

    #[test]
    fn claude_tools_become_steps() {
        let s = step_from_claude("Edit", &json!({"file_path": "/w/.env"}));
        assert_eq!((s.kind, s.paths), (Kind::Edit, vec!["/w/.env".to_string()]));
        let b = step_from_claude("Bash", &json!({"command": "ls"}));
        assert_eq!(b.command.as_deref(), Some("ls"));
        assert!(question_text(
            &json!({"questions": [{"question": "Q?", "options": [{"label": "A"}]}]})
        )
        .contains("Q? (choices: A)"));
    }
}
