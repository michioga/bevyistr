use super::*;

#[test]
fn unexpected_disconnect_drains_logs_then_reports_failure_once() {
    let (tx, rx) = mpsc::channel();
    let handle = SolverProcessHandle::for_test(rx);
    assert!(handle.poll().is_empty()); // an idle, connected worker is not a failure
    for line in 0..600 {
        tx.send(SolverProcessEvent::Output(
            ProcessOutputStream::Stdout,
            line.to_string(),
        ))
        .unwrap();
    }
    drop(tx);
    let mut all = Vec::new();
    for _ in 0..4 {
        let batch = handle.poll();
        assert!(batch.len() <= 256);
        all.extend(batch);
    }
    assert_eq!(all.len(), 601);
    for (index, event) in all[..600].iter().enumerate() {
        assert!(matches!(event, SolverProcessEvent::Output(_, line) if line == &index.to_string()));
    }
    assert!(matches!(&all[600], SolverProcessEvent::SpawnFailed(message)
        if message.contains("stopped unexpectedly")));
    assert!(handle.poll().is_empty());
}

#[test]
fn normal_terminal_events_are_not_replaced_by_disconnect_errors() {
    for event in [
        SolverProcessEvent::Finished(Some(0)),
        SolverProcessEvent::Finished(Some(7)),
        SolverProcessEvent::Finished(None),
        SolverProcessEvent::Stopped,
        SolverProcessEvent::SpawnFailed("known failure".into()),
    ] {
        let (tx, rx) = mpsc::channel();
        let handle = SolverProcessHandle::for_test(rx);
        // More than one UI batch ensures that closing the sender cannot hide
        // a real terminal event behind a large buffered transcript.
        for _ in 0..512 {
            tx.send(SolverProcessEvent::Output(
                ProcessOutputStream::Stdout,
                "line".into(),
            ))
            .unwrap();
        }
        let expected = format!("{event:?}");
        tx.send(event).unwrap();
        drop(tx);
        assert_eq!(handle.poll().len(), 256);
        assert_eq!(handle.poll().len(), 256);
        let end = handle.poll();
        assert_eq!(end.len(), 1);
        assert_eq!(format!("{:?}", end[0]), expected);
        assert!(handle.poll().is_empty());
    }
    let (tx, rx) = mpsc::channel();
    let handle = SolverProcessHandle::for_test(rx);
    tx.send(SolverProcessEvent::Finished(Some(0))).unwrap();
    assert_eq!(handle.poll().len(), 1);
    drop(tx); // disconnect can arrive in a later UI poll than the terminal
    assert!(handle.poll().is_empty());
}
