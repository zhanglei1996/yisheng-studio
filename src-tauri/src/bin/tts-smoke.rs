use std::{
    process,
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::Duration,
};

const SMOKE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, PartialEq, Eq)]
enum SmokeResult {
    Success {
        audio_bytes: usize,
        request_id_present: bool,
    },
    EmptyAudio,
    RequestFailed,
    RuntimeInitFailed,
    InternalFailure,
}

#[derive(Debug, PartialEq, Eq)]
enum WaitResult {
    Complete(SmokeResult),
    TimedOut,
    WorkerStopped,
}

fn wait_for_worker(receiver: &Receiver<SmokeResult>, timeout: Duration) -> WaitResult {
    match receiver.recv_timeout(timeout) {
        Ok(result) => WaitResult::Complete(result),
        Err(RecvTimeoutError::Timeout) => WaitResult::TimedOut,
        Err(RecvTimeoutError::Disconnected) => WaitResult::WorkerStopped,
    }
}

fn recognized_driver(driver: &str) -> Option<&'static str> {
    match driver {
        "aliyun_tts" | "bailian_tts" => Some("aliyun_tts"),
        "iflytek_super_tts" | "iflytek" => Some("iflytek_super_tts"),
        _ => None,
    }
}

fn exit_with(result: WaitResult, driver_label: &str) -> ! {
    let exit_code = match result {
        WaitResult::Complete(SmokeResult::Success {
            audio_bytes,
            request_id_present,
        }) => {
            eprintln!(
                "provider-smoke=ok driver={driver_label} audio_bytes={audio_bytes} request_id_present={request_id_present}"
            );
            0
        }
        WaitResult::Complete(SmokeResult::EmptyAudio) => {
            eprintln!("provider-smoke=failed category=empty-audio");
            1
        }
        WaitResult::Complete(SmokeResult::RequestFailed) => {
            eprintln!("provider-smoke=failed category=request");
            1
        }
        WaitResult::Complete(SmokeResult::RuntimeInitFailed) => {
            eprintln!("provider-smoke=failed category=runtime-init");
            1
        }
        WaitResult::Complete(SmokeResult::InternalFailure) | WaitResult::WorkerStopped => {
            eprintln!("provider-smoke=failed category=internal");
            1
        }
        WaitResult::TimedOut => {
            eprintln!("provider-smoke=failed category=timeout timeout_seconds=120");
            1
        }
    };

    // Intentionally skip destructors. A timed-out Keychain, DNS, TLS, or WebSocket
    // operation can otherwise keep Tokio's blocking pool alive past the deadline.
    process::exit(exit_code)
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(driver) = arguments.next() else {
        eprintln!("usage: tts-smoke <driver> <public-config-json> <credential-reference>");
        process::exit(2);
    };
    let Some(driver_label) = recognized_driver(&driver) else {
        eprintln!("provider-smoke=failed category=unsupported-driver");
        process::exit(2);
    };
    let Some(public_config_json) = arguments.next() else {
        eprintln!("provider-smoke=failed category=missing-public-config");
        process::exit(2);
    };
    let Some(credential_reference) = arguments.next() else {
        eprintln!("provider-smoke=failed category=missing-credential-reference");
        process::exit(2);
    };
    if arguments.next().is_some() {
        eprintln!("provider-smoke=failed category=unexpected-argument");
        process::exit(2);
    }

    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("tts-smoke-worker".into())
        .spawn(move || {
            // The default panic hook may include values from a failed operation.
            // Smoke output deliberately reports only coarse, non-sensitive categories.
            std::panic::set_hook(Box::new(|_| {}));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(_) => return SmokeResult::RuntimeInitFailed,
                };
                let outcome = runtime.block_on(
                    yisheng_studio_lib::tts_provider::smoke_test_keychain_provider(
                        &driver,
                        &public_config_json,
                        &credential_reference,
                    ),
                );
                // Runtime drop can wait for a blocked resolver/Keychain helper. The
                // process is about to exit, so leaking this short-lived runtime keeps
                // a completed provider request from being delayed by cleanup.
                std::mem::forget(runtime);
                match outcome {
                    Ok(audio) if !audio.bytes.is_empty() => SmokeResult::Success {
                        audio_bytes: audio.bytes.len(),
                        request_id_present: audio.request_id.is_some(),
                    },
                    Ok(_) => SmokeResult::EmptyAudio,
                    Err(_) => SmokeResult::RequestFailed,
                }
            }))
            .unwrap_or(SmokeResult::InternalFailure);
            let _ = sender.send(result);
        });

    if worker.is_err() {
        eprintln!("provider-smoke=failed category=worker-init");
        process::exit(1);
    }

    exit_with(wait_for_worker(&receiver, SMOKE_TIMEOUT), driver_label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn wait_for_worker_enforces_deadline_without_joining_worker() {
        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(0);
        let worker = thread::spawn(move || {
            let _ = release_receiver.recv();
            let _ = result_sender.send(SmokeResult::InternalFailure);
        });

        let started = Instant::now();
        assert_eq!(
            wait_for_worker(&result_receiver, Duration::from_millis(20)),
            WaitResult::TimedOut
        );
        assert!(started.elapsed() < Duration::from_secs(1));

        release_sender.send(()).expect("release worker");
        worker.join().expect("join worker");
    }

    #[test]
    fn wait_for_worker_returns_completed_result() {
        let (sender, receiver) = mpsc::sync_channel(1);
        sender
            .send(SmokeResult::Success {
                audio_bytes: 42,
                request_id_present: true,
            })
            .expect("send smoke result");

        assert_eq!(
            wait_for_worker(&receiver, Duration::from_secs(1)),
            WaitResult::Complete(SmokeResult::Success {
                audio_bytes: 42,
                request_id_present: true,
            })
        );
    }

    #[test]
    fn failure_output_has_no_space_for_provider_diagnostics() {
        let sensitive_marker = "must-not-appear-in-output";
        let result = SmokeResult::RequestFailed;

        assert_eq!(result, SmokeResult::RequestFailed);
        assert!(!format!("{result:?}").contains(sensitive_marker));
    }

    #[test]
    fn driver_labels_are_allowlisted() {
        assert_eq!(recognized_driver("bailian_tts"), Some("aliyun_tts"));
        assert_eq!(recognized_driver("iflytek"), Some("iflytek_super_tts"));
        assert_eq!(recognized_driver("value-with-control\nchars"), None);
    }
}
