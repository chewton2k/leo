const assert = require('node:assert/strict');
const S = require('../src/web/schedule.js');

const cases = [];
const test = (name, fn) => cases.push([name, fn]);

test('explicit calendar dates retain the day and deadline', () => {
  const now = new Date(2026, 10, 4, 12);
  for (const date of ['Nov 08', 'November 8', '2026-11-08']) {
    assert.deepEqual(S.quickParse(`math 164 homework due ${date} at 11:59PM`, now), { kind: 'task', title: 'math 164 homework', date: '2026-11-08', time: '23:59' });
  }
  assert.equal(S.quickParse('submit Jan 2', now).date, '2027-01-02');
  assert.equal(S.quickParse('submit Feb 30', now).date, null);
  assert.equal(S.quickParse('submit Nov 8 2027', now).date, '2027-11-08');
});

test('explicit years immediately after a comma keep the whole deadline', () => {
  const now = new Date(2026, 10, 4, 12);
  for (const date of ['Nov 8,2027', 'Nov 8, 2027', 'Nov 8 2027']) {
    assert.deepEqual(S.quickParse(`submit ${date}`, now), { kind: 'task', title: 'submit', date: '2027-11-08', time: null });
  }
});

test('ISO local datetimes validate and keep the whole deadline', () => {
  const now = new Date(2026, 10, 4, 12);
  for (const [clock, expected] of [['00:00', '00:00'], ['11:59', '11:59'], ['23:59', '23:59']]) {
    assert.deepEqual(S.quickParse(`remind me to submit 2026-11-08T${clock}`, now), { kind: 'task', title: 'submit', date: '2026-11-08', time: expected });
  }
  for (const clock of ['24:00', '11:60']) {
    const input = `submit 2026-11-08T${clock}`;
    const parsed = S.quickParse('task: ' + input, now);
    assert.equal(parsed.date, null);
    assert.equal(parsed.time, null);
    assert.equal(parsed.title, input);
  }
});

test('Quick Leo reads a reminder, a question or a note from one line', () => {
  const now = new Date(2026, 9, 19, 10, 0);
  assert.deepEqual(S.quickParse('remind me to email Bob tomorrow at 3pm', now), { kind: 'task', title: 'email Bob', date: '2026-10-20', time: '15:00' });
  assert.deepEqual(S.quickParse('Pay rent on Friday', now), { kind: 'task', title: 'Pay rent', date: '2026-10-23', time: null });
  assert.deepEqual(S.quickParse('todo: submit form next week', now), { kind: 'task', title: 'submit form', date: '2026-10-26', time: null });
  assert.deepEqual(S.quickParse('call mom tonight', now), { kind: 'task', title: 'call mom', date: '2026-10-19', time: '20:00' });
  assert.deepEqual(S.quickParse('standup 9:30', now), { kind: 'task', title: 'standup', date: '2026-10-19', time: '09:30' });
  assert.deepEqual(S.quickParse('dentist in 3 days', now), { kind: 'task', title: 'dentist', date: '2026-10-22', time: null });
  assert.equal(S.quickParse('next monday review notes', now).date, '2026-10-26', 'next Monday is the week after the coming one');
  assert.equal(S.quickParse('gym monday', now).date, '2026-10-19', 'a weekday that is today means today');
  assert.equal(S.quickParse('buy sun cream', now).date, null, 'sun and sat mid-sentence are words, not days');
  assert.equal(S.quickParse('I sat the exam and want to review it', now).date, null);
  assert.equal(S.quickParse('gym sat', now).date, '2026-10-24');
  assert.equal(S.quickParse('brunch on sun with Ana', now).date, '2026-10-25');
  assert.equal(S.quickParse('call mom fri', now).date, '2026-10-23');
  assert.deepEqual(S.quickParse('for sun, remind me to go buy sunscreen', now), { kind: 'task', title: 'go buy sunscreen', date: '2026-10-25', time: null });
  assert.deepEqual(S.quickParse('can you remind me to call mom tomorrow?', now), { kind: 'task', title: 'call mom', date: '2026-10-20', time: null });
  assert.deepEqual(S.quickParse("don't forget to pay rent by fri", now), { kind: 'task', title: 'pay rent', date: '2026-10-23', time: null });
  assert.deepEqual(S.quickParse('sat. pick up the cake', now), { kind: 'task', title: 'pick up the cake', date: '2026-10-24', time: null });
  assert.deepEqual(S.quickParse('remind me on sun to buy sun cream', now), { kind: 'task', title: 'buy sun cream', date: '2026-10-25', time: null });
  assert.deepEqual(S.quickParse('What is a heap?', now), { kind: 'ask', title: 'What is a heap?', date: null, time: null });
  assert.equal(S.quickParse('explain dijkstra', now).kind, 'ask');
  assert.deepEqual(S.quickParse('note: idea for the essay', now), { kind: 'note', title: 'idea for the essay', date: null, time: null });
  assert.equal(S.quickParse('remind me to stretch', now).kind, 'task');
  assert.equal(S.quickParse('remind me to stretch', now).date, null);
});

test('Quick Leo puts what you most likely want first, like Spotlight', () => {
  const now = new Date(2026, 9, 19, 10, 0);
  const notes = [{ id: 'a', title: 'Graph algorithms' }, { id: 'b', title: 'Heaps' }, { id: 'c', title: 'Weekly plan' }];
  const places = [{ name: 'Calendar', hash: '#/calendar' }, { name: 'Settings', hash: '#/settings' }];
  const top = (text, found = notes) => S.quickRank(text, { notes: found, places }, now)[0];
  assert.deepEqual(top('email Bob tomorrow at 3pm'), { kind: 'task', title: 'email Bob', date: '2026-10-20', time: '15:00' });
  assert.deepEqual(top('What is a heap?'), { kind: 'ask', text: 'What is a heap?' });
  assert.deepEqual(top('heaps'), { kind: 'open', id: 'b', title: 'Heaps' });
  assert.deepEqual(top('graph alg'), { kind: 'open', id: 'a', title: 'Graph algorithms' });
  assert.deepEqual(top('cal', []), { kind: 'place', name: 'Calendar', hash: '#/calendar' });
  assert.deepEqual(top('graph', [{ id: 'a', title: 'Graph algorithms' }]), { kind: 'open', id: 'a', title: 'Graph algorithms' }, 'a note named like that beats a page whose later word matches');
  assert.deepEqual(top('note: idea for the essay'), { kind: 'note', title: 'idea for the essay' });
  assert.equal(top('dijkstra versus bellman ford for negative edges', []).kind, 'ask');
  assert.equal(top('how do graph algorithms work').kind, 'ask', 'a question asks even when a note matches');
  const rows = S.quickRank('Pay rent', { notes: [], places }, now);
  assert.deepEqual(rows.map((r) => r.kind), ['task', 'ask', 'search', 'note'], 'every other choice stays one arrow away');
  assert.deepEqual(top('buy milk', []), { kind: 'task', title: 'buy milk', date: null, time: null }, 'a short thing to do is a task');
  assert.equal(top('pick up John from the airport', []).kind, 'task');
  assert.equal(top('call mom tomorrow', []).kind, 'task');
  assert.equal(top('Make a table comparing the 3 lectures', []).kind, 'ask', 'a long request goes to Felix');
  assert.equal(top('summarize chapter 3', []).kind, 'ask');
  assert.equal(top('make a table', []).kind, 'ask', 'a study word goes to Felix even when short');
  assert.equal(top('I keep mixing up the two proofs and want to get them straight before Friday somehow', []).kind, 'task', 'a date still means a reminder');
  assert.equal(top('so I was thinking about the project and how the parts fit together overall', []).kind, 'ask', 'Felix works out anything longer');
  const folders = ['Classes/CS 161', 'Work'];
  const chats = [{ id: 'k1', title: 'Heap questions' }];
  assert.deepEqual(S.quickRank('cs 161', { folders, places }, now)[0], { kind: 'folder', path: 'Classes/CS 161' });
  assert.ok(S.quickRank('cs', { folders, places }, now).some((r) => r.kind === 'folder' && r.path === 'Classes/CS 161'));
  assert.ok(S.quickRank('heap', { chats, places }, now).some((r) => r.kind === 'chat' && r.id === 'k1'), 'past chats that match are listed');
  assert.equal(new Set(S.quickRank('heaps', { notes, places }, now)).size, S.quickRank('heaps', { notes, places }, now).length);
  assert.deepEqual(S.quickRank('dijkstra', { notes, places }, now).find((r) => r.kind === 'search'), { kind: 'search', text: 'dijkstra' }, 'searching every note is always one of the choices');
  const many = S.quickRank('alg', { notes: [{ id: 'x', title: 'alg 1' }, { id: 'y', title: 'alg 2' }, { id: 'z', title: 'alg 3' }], folders: ['Algebra', 'Algorithms'], chats, places }, now);
  assert.ok(many.length <= 8);
  assert.deepEqual(many.slice(-2).map((r) => r.kind), ['task', 'note'], 'adding a task or a note is never pushed off the list');
  assert.deepEqual(S.quickRank('   ', { notes, places }, now), []);
});

test('Calendar descriptions keep their formatting and links but nothing that can run', () => {
  const google = 'Itinerary: Charlton S<br />Monday, Oct 19, 2026 PDT<br/><br/>1:45PM (PDT) - 2:45PM (PDT): Data/Algos Interview<br/><br/>Hi Charlton<br/><br/>You’re all set!<br/><br/><strong>How to connect:</strong><ul><li>Click "Join with Google Meet" then “Join Now”.</li><li>You may see “ask to join the call”.</';
  const html = S.richText(google);
  assert.match(html, /Itinerary: Charlton S<br>Monday, Oct 19, 2026 PDT<br><br>1:45PM/);
  assert.match(html, /<strong>How to connect:<\/strong><ul><li>Click &quot;Join with Google Meet&quot; then “Join Now”.<\/li><li>You may see/);
  assert.ok(html.endsWith('</li></ul>'), 'a cut-off description is closed properly');
  assert.ok(!html.includes('<br />') && !html.includes('</'.repeat(2)));
  const hostile = S.richText('<script>alert(1)</script><img src=x onerror=alert(1)><a href="javascript:alert(1)">bad</a> <a href="https://meet.google.com/abc-defg" onclick="x()">Join</a> <b onmouseover="x">bold</b>');
  assert.ok(!/<script|<img|onerror|onclick|onmouseover|javascript:/i.test(hostile), hostile);
  assert.ok(!hostile.startsWith('alert'), 'a script and its text are dropped');
  assert.match(hostile, /<a href="https:\/\/meet\.google\.com\/abc-defg" target="_blank" rel="noopener noreferrer">Join<\/a>/);
  assert.match(hostile, /<strong>bold<\/strong>/);
  assert.equal(S.richText('Join at https://zoom.us/j/123.\nBring notes & questions'), 'Join at <a href="https://zoom.us/j/123" target="_blank" rel="noopener noreferrer">https://zoom.us/j/123</a>.<br>Bring notes &amp; questions');
  assert.equal(S.richText('Tom &amp; Jerry &lt;3'), 'Tom &amp; Jerry &lt;3');
  assert.equal(S.richText('<p>One</p><p>Two</p>'), '<p>One</p><p>Two</p>');
  assert.equal(S.richText('<a href="mailto:a@b.co">mail</a>'), '<a href="mailto:a@b.co" target="_blank" rel="noopener noreferrer">mail</a>');
});

test('Calendar event details split into organiser, invited and description', () => {
  const d = S.eventDetails('Calendar event: Interview\nPlace: Google Meet\nOrganised by: c_c463e763f192@group.calendar.google.com\nInvited: GoodTime Sync, charlton@ucla.edu, room@resource.calendar.google.com\nDescription:\nLine one\nLine two');
  assert.deepEqual(d, { place: 'Google Meet', organiser: 'A shared calendar', invited: ['GoodTime Sync', 'charlton@ucla.edu'], description: 'Line one\nLine two' });
  assert.deepEqual(S.eventDetails('Calendar event: Lunch'), { place: '', organiser: '', invited: [], description: '' });
});

test('Calendar links preserve descriptions and safely encode plain text', () => {
  const base = { id: 'task-one', title: 'Study', date: '2026-10-16' };
  const details = 'Read chapter 3\nBring café notes; <b>not HTML</b>&x=y';
  const link = new URL(S.googleLink({ ...base, description: details }));
  assert.equal(link.searchParams.get('details'), `${details}\n\nFrom leo · leo-task:task-one`);
  assert.equal(link.searchParams.get('x'), null);
  assert.equal(new URL(S.googleLink(base)).searchParams.get('details'), 'From leo · leo-task:task-one');
});

test('a month grid starts on the chosen weekday and always has six weeks', () => {
  const grid = S.monthGrid(2026, 9, 0);
  assert.equal(grid.length, 42);
  assert.equal(grid[0].key, '2026-09-27');
  assert.equal(grid.find((c) => c.inMonth).key, '2026-10-01');
  assert.equal(S.monthGrid(2026, 9, 1)[0].key, '2026-09-28');
  assert.deepEqual(S.weekOf(new Date(2026, 9, 15), 1), ['2026-10-12', '2026-10-13', '2026-10-14', '2026-10-15', '2026-10-16', '2026-10-17', '2026-10-18']);
});

test('tasks and events land on their days; overdue and undated are grouped; a linked event shows once', () => {
  const tasks = [
    { id: 'a', title: 'Submit lab', date: '2026-10-13', time: null, done: false },
    { id: 'b', title: 'Exam', date: '2026-10-15', time: '09:00:00', done: false },
    { id: 'c', title: 'Ask tutor', date: null, done: false },
    { id: 'd', title: 'Old', date: '2026-10-01', done: true },
    { id: 'e', title: 'Sent', date: '2026-10-16', time: null, done: false },
  ];
  const events = [
    { id: 'x', title: 'Trip', all_day: true, date: '2026-10-15', end_date: '2026-10-17', start: '2026-10-15T07:00:00Z' },
    { id: 'y', title: 'Meeting', start: '2026-10-15T17:00:00Z', end: '2026-10-15T18:00:00Z' },
    { id: 'z', title: 'Due: Sent', task: 'e', start: '2026-10-16T06:30:00Z' },
  ];
  const p = S.plan(tasks, events, '2026-10-14');
  assert.deepEqual(p.overdue.map((t) => t.id), ['a']);
  assert.deepEqual(p.undated.map((t) => t.id), ['c']);
  const day15 = p.byDay.get('2026-10-15');
  assert.deepEqual(day15.allDay.map((i) => i.event.title), ['Trip']);
  assert.equal(day15.timed.length, 2);
  assert.deepEqual(p.byDay.get('2026-10-16').allDay.map((i) => i.event ? i.event.title : i.task.title), ['Sent', 'Trip']);
  assert.ok(![...p.byDay.values()].some((d) => d.timed.some((i) => i.event && i.event.id === 'z')), 'the Google copy of a task is not shown twice');
  assert.equal(p.byDay.get('2026-10-17'), undefined, 'an all-day end date is exclusive');
});

test('only timed tasks and events that are not free, declined or all-day are busy', () => {
  const event = (extra) => ({ type: 'event', event: { all_day: false, ...extra } });
  assert.equal(S.busy(event({})), true);
  assert.equal(S.busy(event({ reply: 'tentative' })), true);
  assert.equal(S.busy(event({ reply: 'declined' })), false);
  assert.equal(S.busy(event({ free: true })), false);
  assert.equal(S.busy(event({ all_day: true })), false);
  assert.equal(S.busy({ type: 'task', task: { time: '09:00:00' } }), true);
  assert.equal(S.busy({ type: 'task', task: { time: null } }), false);
});

test('quick dates are today, tomorrow, the coming Friday and next Monday', () => {
  const thursday = new Date(2026, 9, 15);
  assert.deepEqual(S.quickDates(thursday).map((q) => q.key), ['2026-10-15', '2026-10-16', '2026-10-16', '2026-10-19']);
  const friday = new Date(2026, 9, 16);
  assert.deepEqual(S.quickDates(friday).slice(2).map((q) => [q.label, q.key]), [['Next Fri', '2026-10-23'], ['Next week', '2026-10-19']]);
});

test('a Google Calendar link is prefilled for all-day, deadline and meeting tasks', () => {
  const read = (url) => Object.fromEntries(new URL(url).searchParams);
  const allDay = read(S.googleLink({ id: 'abcdef1234567890', title: 'Biology final', date: '2026-12-16', kind: 'exam' }, 'America/Los_Angeles'));
  assert.equal(allDay.dates, '20261216/20261217');
  assert.equal(allDay.ctz, 'America/Los_Angeles');
  assert.equal(allDay.details, 'From leo · leo-task:abcdef123456');
  const due = read(S.googleLink({ id: 't1', title: 'Submit lab', date: '2026-10-16', time: '00:10:00', kind: 'assignment', note: { title: 'Chem' } }));
  assert.equal(due.text, 'Due: Submit lab');
  assert.equal(due.dates, '20261015T234000/20261016T001000', 'a deadline is the half hour before it, even across midnight');
  assert.equal(due.details, 'From leo · Chem · leo-task:t1');
  const meet = read(S.googleLink({ id: 't2', title: 'Project meeting', date: '2026-10-16', time: '10:00:00', end: '11:30:00', kind: 'meeting' }));
  assert.deepEqual([meet.text, meet.dates], ['Project meeting', '20261016T100000/20261016T113000']);
  assert.equal(S.googleLink({ id: 't3', title: 'x', date: null }), null);
  assert.equal(S.googleDay('2026-10-08'), 'https://calendar.google.com/calendar/r/day/2026/10/8');
});

test('a repeating task shows on its weekdays from its start, done day by day, never overdue', () => {
  const gym = { id: 'g', title: 'Gym', date: '2026-10-12', repeat: [0, 2], done_on: ['2026-10-12'] };
  const p = S.plan([gym], [], '2026-10-20', { from: '2026-10-05', to: '2026-10-25' });
  const days = [...p.byDay.keys()].sort();
  assert.deepEqual(days, ['2026-10-12', '2026-10-14', '2026-10-19', '2026-10-21']);
  assert.equal(p.byDay.get('2026-10-12').allDay[0].task.done, true);
  assert.equal(p.byDay.get('2026-10-14').allDay[0].task.done, false);
  assert.equal(p.byDay.get('2026-10-14').allDay[0].task.on, '2026-10-14');
  assert.deepEqual(p.overdue, []);
  assert.deepEqual(p.undated, []);
  assert.equal(S.weekdayOf('2026-10-12'), 0);
  assert.equal(S.weekdayOf('2026-10-18'), 6);
  assert.equal(S.nextOpen(gym, '2026-10-12', '2026-10-19').date, '2026-10-14');
  assert.equal(S.repeatLabel({ repeat: [0, 1, 2, 3, 4] }), 'Every weekday');
  assert.equal(S.repeatLabel({ repeat: [6, 5, 0, 1, 2, 3, 4] }), 'Every day');
  assert.equal(S.rrule({ repeat: [4, 0, 2] }), 'RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR;WKST=MO');
});

test('Google gets a weekly rule starting on the first repeat day', () => {
  const link = new URL(S.googleLink({ id: 'g', title: 'Gym', date: '2026-10-13', repeat: [2], kind: 'other' }));
  assert.equal(link.searchParams.get('recur'), 'RRULE:FREQ=WEEKLY;BYDAY=WE;WKST=MO');
  assert.equal(link.searchParams.get('dates'), '20261014/20261015');
});

test('every other week keeps whole weeks from the start week', () => {
  const gym = { id: 'g', title: 'Gym', date: '2026-11-04', repeat: [0, 2], every: 2 };
  const days = S.occurrences(gym, '2026-11-01', '2026-11-30').map((o) => o.date);
  assert.deepEqual(days, ['2026-11-04', '2026-11-16', '2026-11-18', '2026-11-30']);
  assert.equal(S.repeatsOn(gym, '2026-11-02'), false);
});

test('a monthly task keeps its day, skips months without it, and every two months skips alternate ones', () => {
  const rent = { id: 'r', title: 'Rent', date: '2026-01-31', monthly: true };
  assert.deepEqual(S.occurrences(rent, '2026-01-01', '2026-06-30').map((o) => o.date), ['2026-01-31', '2026-03-31', '2026-05-31']);
  assert.equal(S.repeats(rent), true);
  const bills = { ...rent, every: 2 };
  assert.deepEqual(S.occurrences(bills, '2026-01-01', '2026-09-30').map((o) => o.date), ['2026-01-31', '2026-03-31', '2026-05-31', '2026-07-31']);
  assert.equal(S.repeatsOn(bills, '2026-08-31'), false);
});

test('nothing repeats after its until day, and moves and done days follow the real occurrences', () => {
  const gym = { id: 'g', title: 'Gym', date: '2026-11-02', repeat: [0], every: 2, until: '2026-11-30', done_on: ['2026-11-16'], moved: { '2026-11-09': { date: '2026-11-10', time: null }, '2026-11-16': { date: '2026-11-17', time: null } } };
  const out = S.occurrences(gym, '2026-11-01', '2026-12-31');
  assert.deepEqual(out.map((o) => o.date), ['2026-11-02', '2026-11-17', '2026-11-30']);
  assert.deepEqual(out.map((o) => o.done), [false, true, false]);
  assert.equal(S.repeatsOn(gym, '2026-12-14'), false);
});

test('repeat words, rules and Google links carry the interval, month day and last day', () => {
  const gym = { id: 'g', title: 'Gym', date: '2026-11-02', repeat: [0, 2], every: 2, until: '2026-12-19' };
  assert.equal(S.repeatLabel(gym, 'en-US'), 'Every 2 weeks on Mon, Wed until Dec 19');
  assert.equal(S.rrule(gym), 'RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;WKST=MO;UNTIL=20261219');
  const timed = S.rrule({ ...gym, time: '09:00:00' });
  assert.equal(timed, `RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;WKST=MO;UNTIL=${new Date(2026, 11, 19, 23, 59, 59).toISOString().replace(/[-:]|\.\d+/g, '')}`);
  const rent = { id: 'r', title: 'Rent', date: '2026-11-22', monthly: true };
  assert.equal(S.repeatLabel(rent, 'en-US'), 'Monthly on the 22nd');
  assert.equal(S.repeatLabel({ ...rent, every: 3, until: '2027-02-01' }, 'en-US'), 'Every 3 months on the 22nd until Feb 1');
  assert.equal(S.rrule({ ...rent, every: 3 }), 'RRULE:FREQ=MONTHLY;INTERVAL=3;BYMONTHDAY=22');
  const link = new URL(S.googleLink(rent));
  assert.equal(link.searchParams.get('recur'), 'RRULE:FREQ=MONTHLY;BYMONTHDAY=22');
  assert.equal(S.repeatLabel({ date: '2026-11-02' }), '');
  assert.equal(S.rrule({ date: '2026-11-02' }), '');
});

test('a moved day of a repeating task shows where it went, keeps its name, and frees its old day', () => {
  const gym = { id: 'g', title: 'Gym', date: '2026-11-02', time: '09:00:00', repeat: [0, 2], done_on: ['2026-11-04'], moved: { '2026-11-04': { date: '2026-11-05', time: '15:00:00', end: '16:00:00' }, '2026-11-03': { date: '2026-11-06', time: null } } };
  const p = S.plan([gym], [], '2026-11-03', { from: '2026-11-02', to: '2026-11-08' });
  assert.equal(p.byDay.has('2026-11-04'), false);
  const moved = p.byDay.get('2026-11-05').timed[0].task;
  assert.deepEqual([moved.on, moved.time, moved.end, moved.done, moved.shifted], ['2026-11-04', '15:00:00', '16:00:00', true, true]);
  assert.equal(p.byDay.has('2026-11-06'), false, 'a move of a day that is not one of its days is ignored');
  assert.equal(p.byDay.get('2026-11-02').timed[0].task.shifted, false);
  const later = S.occurrences(gym, '2026-11-05', '2026-11-12');
  assert.deepEqual(later.map((o) => o.date), ['2026-11-05', '2026-11-09', '2026-11-11'], 'a move into the range counts even when its old day is outside it');
  assert.equal(S.nextOpen(gym, '2026-11-04', '2026-11-12').date, '2026-11-09', 'the done moved day is skipped');
});

test('day, week and month views cover their days', () => {
  assert.deepEqual(S.daysOf('day', '2026-11-03'), ['2026-11-03']);
  assert.deepEqual(S.daysOf('week', '2026-11-04', 1), ['2026-11-02', '2026-11-03', '2026-11-04', '2026-11-05', '2026-11-06', '2026-11-07', '2026-11-08']);
  const month = S.daysOf('month', '2026-11-20', 1);
  assert.equal(month.length, 42);
  assert.equal(month[0], '2026-10-26');
  assert.ok(month.length <= 43, 'a month fits the schedule window');
});

test('times snap to quarter hours and spans never end before they start', () => {
  assert.equal(S.minutesOf('09:30:00'), 570);
  assert.equal(S.timeOf(570), '09:30');
  assert.equal(S.timeOf(24 * 60 + 5), '23:59');
  assert.equal(S.snap(568), 570);
  assert.equal(S.snap(562), 555);
  const due = { type: 'task', task: { time: '23:50', end: null } };
  assert.deepEqual(S.spanOn(due, '2026-11-03'), { start: 1430, end: 1440 });
  assert.deepEqual(S.spanOn({ type: 'task', task: { time: '10:00', end: '11:30' } }, '2026-11-03'), { start: 600, end: 690 });
  const night = { type: 'event', event: { start: new Date(2026, 10, 3, 22, 0).toISOString(), end: new Date(2026, 10, 4, 2, 0).toISOString() } };
  assert.deepEqual(S.spanOn(night, '2026-11-03'), { start: 1320, end: 1440 });
  assert.deepEqual(S.spanOn(night, '2026-11-04'), { start: 0, end: 120 });
  assert.deepEqual(S.eventDays(night.event), ['2026-11-03', '2026-11-04']);
  assert.deepEqual(S.eventDays({ start: new Date(2026, 10, 3, 9).toISOString(), end: new Date(2026, 10, 3, 10).toISOString() }), ['2026-11-03']);
});

test('overlapping items share the width, separate clusters do not', () => {
  const cols = S.dayColumns([{ start: 540, end: 600 }, { start: 570, end: 660 }, { start: 600, end: 630 }, { start: 700, end: 760 }]);
  assert.deepEqual(cols, [{ col: 0, cols: 2 }, { col: 1, cols: 2 }, { col: 0, cols: 2 }, { col: 0, cols: 1 }]);
  assert.deepEqual(S.dayColumns([]), []);
  assert.deepEqual(S.lanes([{ a: 0, b: 2 }, { a: 1, b: 1 }, { a: 3, b: 4 }, { a: 2, b: 5 }]), [0, 1, 0, 1]);
});

test('moving every day of a repeating task shifts its weekdays', () => {
  assert.deepEqual(S.shiftRepeat([0, 2], 1), [1, 3]);
  assert.deepEqual(S.shiftRepeat([6], 1), [0]);
  assert.deepEqual(S.shiftRepeat([0, 2], -8), [1, 6]);
});

const said = (parts) => parts.map((p) => (typeof p === 'string' ? p : `**${p.title}**`)).join('');
const at = (h, m = 0) => S.timeLabel(`${h}:${m}`, 'en-US');
const iso = (day, h, m = 0) => new Date(2026, 10, day, h, m).toISOString();
const sunday = (h, m = 0) => new Date(2026, 10, 15, h, m);
const line = (o) => said(S.dateline({ locale: 'en-US', notes: 4, ...o }));

test('the home dateline: first day, an event on now, and early morning', () => {
  assert.equal(line({ notes: 0, now: sunday(10) }), 'Nothing here yet. Type in the bar to jot a note or a task.');
  assert.equal(line({ notes: null, now: sunday(10) }), 'Nothing scheduled today.');
  const events = [{ title: 'Seminar', start: iso(15, 10), end: iso(15, 11) }];
  const tasks = [{ id: 'a', title: 'Problem set review', date: '2026-11-15', time: '14:00' }, { id: 'b', title: 'Haircut', date: '2026-11-15', time: '09:00', done: true }, { id: 'c', title: 'Email the dataset authors', date: '2026-11-12' }];
  assert.equal(line({ tasks, events, now: sunday(10, 20) }), `**Seminar** until ${at(11)}. Then **Problem set review** at ${at(14)}.`);
  assert.equal(line({ events, now: sunday(10, 20) }), `**Seminar** until ${at(11)}.`);
  assert.equal(line({ events: [{ title: 'Seminar', start: iso(15, 10), end: iso(15, 11) }], now: sunday(1) }), `Today starts with **Seminar** at ${at(10)}.`);
});

test('the home dateline counts today’s tasks and adds earlier ones to the first clause', () => {
  const day = (n, time) => Array.from({ length: n }, (_, i) => ({ id: `t${i}`, title: i ? `Task ${i}` : 'Lab meeting', date: '2026-11-15', time: i ? null : time }));
  const earlier = [{ id: 'e1', title: 'Return library books', date: '2026-11-14' }, { id: 'e2', title: 'Email', date: '2026-11-12' }];
  assert.equal(line({ tasks: [...day(8, '09:00'), ...earlier], now: sunday(8, 40) }), `Eight today and two from earlier. First up is **Lab meeting** at ${at(9)}.`);
  assert.equal(line({ tasks: day(12), now: sunday(8) }), '12 today.');
  assert.equal(line({ tasks: [{ id: 'h', title: 'Haircut', date: '2026-11-15', time: '11:00' }], now: sunday(9) }), `One left today: **Haircut** at ${at(11)}.`);
  assert.equal(line({ tasks: [{ id: 's', title: 'Send draft to advisor', date: '2026-11-15' }, earlier[0]], now: sunday(9) }), 'One left today and one from earlier: **Send draft to advisor**.');
  assert.equal(line({ tasks: [{ id: 'p', title: 'Problem set review', date: '2026-11-15', time: '14:00' }, { id: 's', title: 'Send draft', date: '2026-11-15' }], now: sunday(9) }), `Two left today. **Problem set review** is at ${at(14)}.`);
  assert.equal(line({ tasks: day(2).map((t) => ({ ...t, time: null })), now: sunday(21) }), 'Two left today, none at a set time.');
});

test('the home dateline when today is done, quiet or only earlier tasks remain', () => {
  const done = ['A', 'B', 'C'].map((title) => ({ id: title, title, date: '2026-11-15', done: true }));
  const seminar = [{ title: 'Seminar', start: iso(16, 10), end: iso(16, 11) }];
  assert.equal(line({ tasks: done, events: seminar, now: sunday(19) }), `All three done. Tomorrow starts with **Seminar** at ${at(10)}.`);
  assert.equal(line({ tasks: done, now: sunday(19) }), 'All three done.');
  assert.equal(line({ tasks: [done[0]], now: sunday(19) }), 'Today’s one task is done.');
  assert.equal(line({ tasks: [{ id: 'e', title: 'Old', date: '2026-11-10' }, { id: 'f', title: 'Older', date: '2026-11-09' }], now: sunday(9) }), 'Nothing new today. Two from earlier are still open.');
  assert.equal(line({ tasks: [{ id: 'e', title: 'Old', date: '2026-11-10' }], now: sunday(9) }), 'Nothing new today. One from earlier is still open.');
  assert.equal(line({ tasks: [{ id: 'e', title: 'Old', date: '2026-11-10', done: true }], now: sunday(9) }), 'Nothing scheduled today.');
});

test('the home dateline looks ahead a week, past the weekend and into tomorrow evening', () => {
  const monday = [{ title: 'Seminar', start: iso(16, 10), end: iso(16, 11) }];
  assert.equal(line({ events: monday, now: sunday(9) }), `Nothing on until **Seminar**, Monday at ${at(10)}.`);
  assert.equal(line({ events: monday, now: new Date(2026, 10, 14, 9) }), `Nothing on until **Seminar**, Monday at ${at(10)}.`);
  const friday = new Date(2026, 10, 13, 9);
  assert.equal(line({ events: [{ title: 'Seminar', start: iso(17, 10), end: iso(17, 11) }], now: new Date(2026, 10, 16, 9) }), `Nothing scheduled today. Next is **Seminar**, tomorrow at ${at(10)}.`);
  assert.equal(line({ events: [{ title: 'Seminar', start: iso(17, 10), end: iso(17, 11) }], now: new Date(2026, 10, 16, 18) }), `Nothing scheduled today. Tomorrow starts with **Seminar** at ${at(10)}.`);
  assert.equal(line({ events: [{ title: 'Seminar', start: iso(18, 10), end: iso(18, 11) }], now: new Date(2026, 10, 16, 9) }), `Nothing scheduled today. Next is **Seminar**, Wednesday at ${at(10)}.`);
  assert.equal(line({ events: [{ title: 'Far', start: iso(25, 10), end: iso(25, 11) }], now: friday }), 'Nothing scheduled today.');
  assert.equal(line({ events: [{ title: 'Seminar', start: iso(15, 14), end: iso(15, 15) }], now: sunday(9) }), `**Seminar** at ${at(14)}.`);
  assert.equal(line({ events: [{ title: 'Conference', all_day: true, date: '2026-11-15', end_date: '2026-11-16' }], now: sunday(9) }), '**Conference** all day.');
});

test('the home dateline shortens titles, skips events made from tasks and keeps done rows for today', () => {
  const long = 'A very long task title that keeps on going past forty characters';
  assert.equal(line({ tasks: [{ id: 'l', title: long, date: '2026-11-15' }], now: sunday(9) }), `One left today: **${Array.from(long).slice(0, 40).join('').trimEnd()}…**.`);
  const tasks = [{ id: 'abc', title: 'Haircut', date: '2026-11-15', time: '11:00' }];
  assert.equal(line({ tasks, events: [{ title: 'Haircut (calendar copy)', start: iso(15, 9), end: iso(15, 12), task: 'abc' }], now: sunday(10) }), `One left today: **Haircut** at ${at(11)}.`);
  const { earlier, today } = S.homeDay([{ id: 'x', title: 'Done', date: '2026-11-15', done: true }, { id: 'y', title: 'Open', date: '2026-11-15', time: '08:00' }, { id: 'z', title: 'Old done', date: '2026-11-10', done: true }, { id: 'w', title: 'Old', date: '2026-11-10' }, { id: 'r', title: 'Repeat', repeat: [6], done_on: ['2026-11-15'] }], sunday(9));
  assert.deepEqual(earlier.map((t) => t.id), ['w']);
  assert.deepEqual(S.homeDay([{ id: 'z', title: 'Just ticked', date: '2026-11-10', done: true }], sunday(9), new Set(['z'])).earlier.map((t) => t.id), ['z']);
  assert.deepEqual(today.map((t) => [t.id, Boolean(t.done)]), [['y', false], ['x', true], ['r', true]]);
  for (const text of [line({ tasks: [{ id: 'e', title: 'Old', date: '2026-11-10' }], now: sunday(9) }), line({ tasks: [{ id: 'q', title: 'Q', date: '2026-11-15' }, { id: 'e', title: 'Old', date: '2026-11-10' }], now: sunday(9) })]) {
    assert.doesNotMatch(text, /overdue|late|behind|missed|!/i);
  }
});

test('task dates read as words: today, tomorrow, a weekday within a week either way, then the date', () => {
  const now = new Date(2026, 10, 12, 14, 20);
  const word = (key) => S.dayWord(key, now, 'en-US');
  assert.equal(word('2026-11-12'), 'Today');
  assert.equal(word('2026-11-13'), 'Tomorrow');
  assert.equal(word('2026-11-17'), 'Tue');
  assert.equal(word('2026-11-18'), 'Wed');
  assert.equal(word('2026-11-19'), 'Nov 19');
  assert.equal(word('2026-11-11'), 'Wed');
  assert.equal(word('2026-11-10'), 'Tue');
  assert.equal(word('2026-11-06'), 'Fri');
  assert.equal(word('2026-11-05'), 'Nov 5');
  assert.equal(word('2026-11-03'), 'Nov 3');
  assert.equal(S.dayWord('2026-11-02', new Date(2026, 10, 1, 23, 30), 'en-US'), 'Tomorrow', 'a clock change in between still counts whole days');
  assert.equal(S.dayName('2026-11-17', 'en-US'), 'Tuesday, November 17', 'the full date screen readers and tooltips get');
});

test('a tally draws one stroke per task, gates every fifth and never re-rolls', () => {
  assert.deepEqual(S.tally(0), { marks: [], width: 2.5 });
  assert.equal(S.tally(1).marks.length, 1);
  assert.equal(S.tally(1).width, 7);
  assert.equal(S.tally(3).width, 16);
  assert.equal(S.tally(4).width, 20.5);
  const five = S.tally(5);
  assert.equal(five.width, 26);
  assert.match(five.marks[4].d, /^M1 9\.5/, 'the fifth is the gate across the four');
  const seven = S.tally(7);
  assert.equal(seven.width, 35);
  assert.deepEqual(seven.marks.slice(5).map((m) => m.x), [23.5, 23.5]);
  assert.deepEqual(seven.marks.slice(0, 5), five.marks);
  assert.deepEqual(S.tally(7), seven);
  assert.deepEqual(S.tally(-2).marks, []);
});

test('a meeting link prefers a known meeting host over an earlier generic link and is empty without one', () => {
  const event = (extra) => ({ title: 'Sync', location: '', context: '', ...extra });
  assert.equal(S.meetingLink(event({ location: 'https://example.com/agenda', context: 'Join: https://us02web.zoom.us/j/123?pwd=a&amp;b=2.' })), 'https://us02web.zoom.us/j/123?pwd=a&b=2');
  assert.equal(S.meetingLink(event({ context: '<a href="https://meet.google.com/abc-defg-hij">Join</a> or https://example.com' })), 'https://meet.google.com/abc-defg-hij');
  assert.equal(S.meetingLink(event({ location: 'https://example.com/room', context: 'see https://notes.example.org/x' })), 'https://example.com/room');
  assert.equal(S.meetingLink(event({ location: 'https://evilzoom.us/j/1', context: 'https://teams.live.com/meet/9' })), 'https://teams.live.com/meet/9');
  assert.equal(S.meetingLink(event({ location: 'DC 1351', context: 'Bring a laptop. http://zoom.us/j/1' })), '');
  assert.equal(S.meetingLink(event({})), '');
});

test('items overlap only when both are busy, and each names the others it overlaps', () => {
  const at = (h, m = 0) => new Date(2026, 10, 3, h, m).toISOString();
  const event = (title, start, end, extra = {}) => ({ type: 'event', event: { title, start, end, all_day: false, ...extra } });
  const standup = event('Standup', at(10), at(10, 30));
  const sync = event('Sync', at(10, 15), at(11));
  const free = event('Focus', at(10), at(11), { free: true });
  const declined = event('Skipped', at(10), at(11), { reply: 'declined' });
  const lunch = event('Lunch', at(12), at(13));
  const due = { type: 'task', task: { title: 'Call', date: '2026-11-03', time: '10:20:00', end: null } };
  const found = S.overlaps([standup, sync, free, declined, lunch, due], '2026-11-03');
  assert.deepEqual([...found.keys()].map((i) => i.event ? i.event.title : i.task.title), ['Standup', 'Sync', 'Call']);
  assert.deepEqual(found.get(standup).map((i) => i.event ? i.event.title : i.task.title), ['Sync', 'Call']);
  assert.equal(found.has(free) || found.has(declined) || found.has(lunch), false);
  assert.equal(S.overlaps([standup, lunch], '2026-11-03').size, 0);
});

test('free slots subtract busy time on a step grid from the next step, skip the past and non-working days', () => {
  const hours = { days: [0, 1, 2, 3, 4], from: '09:00', to: '17:00' };
  const at = (d, h, m = 0) => new Date(2026, 10, d, h, m);
  const times = (slots) => slots.map((s) => `${s.start.getHours()}:${String(s.start.getMinutes()).padStart(2, '0')}`);
  const out = S.freeSlots({ busy: [], days: ['2026-11-06', '2026-11-07', '2026-11-09'], hours, minutes: 30, now: at(1, 8) });
  assert.deepEqual(out.map((d) => d.day), ['2026-11-06', '2026-11-09'], 'Saturday is skipped');
  assert.equal(out[0].slots.length, 16);
  assert.equal(out[0].slots[0].end.getTime() - out[0].slots[0].start.getTime(), 30 * 60000);
  const today = S.freeSlots({ busy: [{ start: at(3, 11), end: at(3, 12, 15) }, { start: at(3, 15), end: at(3, 15, 30) }], days: ['2026-11-03'], hours, minutes: 30, now: at(3, 10, 12) })[0];
  assert.deepEqual(times(today.slots), ['10:30', '12:30', '13:00', '13:30', '14:00', '14:30', '15:30', '16:00', '16:30']);
  const long = S.freeSlots({ busy: [{ start: at(3, 10), end: at(3, 10, 45) }, { start: at(3, 11, 30), end: at(3, 16) }], days: ['2026-11-03'], hours, minutes: 60, now: at(3, 8) })[0];
  assert.deepEqual(times(long.slots), ['9:00', '16:00'], 'a gap shorter than the length gives no slot');
  assert.deepEqual(S.freeSlots({ busy: [], days: ['2026-11-03'], hours, minutes: 30, now: at(3, 17) })[0].slots, []);
  assert.deepEqual(times(S.freeSlots({ busy: [{ start: at(3, 9, 30), end: at(3, 10) }], days: ['2026-11-03'], hours: { ...hours, to: '10:30' }, minutes: 30, step: 15, now: at(3, 8) })[0].slots), ['9:00', '10:00']);
});

let failed = 0;
for (const [name, fn] of cases) {
  try {
    fn();
    console.log(`ok   ${name}`);
  } catch (e) {
    failed++;
    console.log(`FAIL ${name}\n     ${e.message.split('\n').join('\n     ')}`);
  }
}
console.log(`${cases.length - failed} passed, ${failed} failed`);
if (failed) process.exit(1);
