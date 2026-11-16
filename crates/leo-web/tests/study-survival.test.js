'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
require('../src/web/chat.js');
const C = globalThis.leoChat;

test('Study keeps each quiz kind and follows a marked answer with an explicit next step', () => {
  const questions = [
    { kind: 'multiple_choice', question: 'Which structure preserves arrival order?', options: ['Stack', 'Queue'], answer: 'Queue' },
    { kind: 'fill_blank', question: 'A FIFO structure is a ___.', answer: 'queue' },
    { kind: 'free_response', question: 'Explain why BFS uses a queue.', answer: 'It explores nodes in discovery order.' },
  ];
  for (const question of questions) {
    assert.ok(C.QUIZ_KINDS[question.kind], question.kind);
    const marked = C.quizSay(question, question.answer, true, true);
    assert.match(marked, /\[\[correct\]\]/);
    assert.doesNotMatch(marked, /ask the next question with the quiz tool/);
    assert.match(C.heldSay({ ...question, given: question.answer, correct: true }, false), /Ask the next question with the quiz tool/);
  }
});

test('Study sessions retain score and completion independently of a coding workspace', () => {
  const session = C.startSession(['Queue order', 'Visited nodes']);
  C.scoreSession(session, { idea: 1 }, true);
  C.scoreSession(session, { idea: 2 }, false);
  assert.equal(C.sessionBody(session).closed.every(Boolean), false);
  C.scoreSession(session, { idea: 2 }, false);
  assert.equal(C.sessionBody(session).closed.every(Boolean), true);
  assert.deepEqual(C.sessionSummary(session), { total: 2, covered: 2, first: 1, review: ['Visited nodes'], skipped: [] });
});
