export class ReaderState {
  constructor() {
    this.key = null;
    this.generation = 0;
    this.manualList = false;
    this.messages = new Map();
  }
  autoOpen(list) {
    return !this.manualList && list.length === 1 ? list[0].key : null;
  }
  open(key) {
    this.key = key;
    this.messages.clear();
    return ++this.generation;
  }
  back() {
    this.manualList = true;
    this.open(null);
  }
  turns() {
    const turns = [];
    for (const message of [...this.messages.values()].sort((a, b) => a.order - b.order)) {
      if (!turns.length || message.startsTurn) turns.push([]);
      turns[turns.length - 1].push(message);
    }
    return turns;
  }
  changes(generation, messages) {
    if (generation !== this.generation) return [];
    const changed = [];
    for (const message of messages) {
      const previous = this.messages.get(message.id);
      if (!previous || previous.text !== message.text || previous.role !== message.role || previous.order !== message.order || previous.startsTurn !== message.startsTurn) {
        this.messages.set(message.id, message);
        changed.push(message);
      }
    }
    return changed;
  }
}
