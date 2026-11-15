let pageArrival = null;

function stopPageArrival() {
  if (pageArrival) pageArrival.cancel();
  pageArrival = null;
}

function arriveOnPage(element) {
  stopPageArrival();
  if (stillMotion() || typeof element.animate !== 'function') return;
  const run = element.animate([{ opacity: .6 }, { opacity: 1 }], { duration: 140, easing: EASE_OUT });
  pageArrival = run;
  run.onfinish = () => { if (pageArrival === run) pageArrival = null; };
}
