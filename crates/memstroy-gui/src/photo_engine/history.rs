use crate::photo_engine::types::PhotoEngineDocument;

pub fn undo(doc: &mut PhotoEngineDocument) -> bool {
    let Some(previous) = doc.undo_stack.pop() else { return false; };
    let current = doc.snapshot();
    doc.redo_stack.push(current);
    doc.restore(previous);
    true
}

pub fn redo(doc: &mut PhotoEngineDocument) -> bool {
    let Some(next) = doc.redo_stack.pop() else { return false; };
    let current = doc.snapshot();
    doc.undo_stack.push(current);
    doc.restore(next);
    true
}

pub fn clear_history(doc: &mut PhotoEngineDocument) {
    doc.undo_stack.clear();
    doc.redo_stack.clear();
}
