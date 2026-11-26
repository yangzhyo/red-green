import test from 'node:test';
import assert from 'node:assert/strict';
import { ReaderState } from '../ui/reader-model.js';

test('空目录等待，唯一会话自动打开，主动返回时留在列表', () => {
  const state = new ReaderState();
  assert.equal(state.autoOpen([]), null);
  assert.equal(state.autoOpen([{key:'a'}]), 'a');
  state.open('a');
  state.back();
  assert.equal(state.autoOpen([{key:'a'}]), null);
  assert.equal(new ReaderState().autoOpen([{key:'a'}, {key:'b'}]), null);
});

test('切换后忽略旧请求，同一回复增长时只更新原消息，重复响应不重绘', () => {
  const state = new ReaderState();
  const old = state.open('a');
  const current = state.open('b');
  const message = {id:'1', role:'assistant', text:'一段'};
  assert.deepEqual(state.changes(old, [message]), []);
  assert.deepEqual(state.changes(current, [message]), [message]);
  const grown = {...message, text:'一段\n二段'};
  assert.deepEqual(state.changes(current, [grown]), [grown]);
  assert.deepEqual(state.changes(current, [grown]), []);
  assert.equal(state.messages.size, 1);
});
