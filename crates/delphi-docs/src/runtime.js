(function () {
  'use strict';
  // Delphi page runtime. Audited code shipped in the binary; generated pages carry it unchanged.
  // It reads nothing but the page it is in, makes no requests, builds no code, and leaves the page where it is.
  var doc = document;

  function text(el) { return (el.textContent || '').trim(); }
  function num(s) {
    var t = s.replace(/[,$%\s]/g, '');
    return t !== '' && isFinite(t) ? parseFloat(t) : NaN;
  }

  // Sortable tables: click or press Enter on a column heading.
  doc.querySelectorAll('table[data-sortable]').forEach(function (table) {
    var heads = table.querySelectorAll('thead th');
    heads.forEach(function (th, col) {
      th.tabIndex = 0;
      th.setAttribute('aria-sort', 'none');
      function sort() {
        var dir = th.getAttribute('aria-sort') === 'ascending' ? 'descending' : 'ascending';
        heads.forEach(function (h) { h.setAttribute('aria-sort', 'none'); });
        th.setAttribute('aria-sort', dir);
        var body = table.tBodies[0];
        var rows = Array.prototype.slice.call(body.rows);
        rows.sort(function (a, b) {
          var x = text(a.cells[col]);
          var y = text(b.cells[col]);
          var nx = num(x);
          var ny = num(y);
          var r = (!isNaN(nx) && !isNaN(ny)) ? nx - ny : x.localeCompare(y, undefined, { numeric: true, sensitivity: 'base' });
          return dir === 'ascending' ? r : -r;
        });
        rows.forEach(function (r) { body.appendChild(r); });
      }
      th.addEventListener('click', sort);
      th.addEventListener('keydown', function (e) {
        if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); sort(); }
      });
    });
  });

  // Filter boxes: hide the rows of the named table that do not contain the typed text.
  doc.querySelectorAll('input[data-filter]').forEach(function (input) {
    var table = doc.getElementById(input.getAttribute('data-filter'));
    if (!table) { return; }
    input.addEventListener('input', function () {
      var q = input.value.toLowerCase();
      Array.prototype.forEach.call(table.tBodies[0].rows, function (r) {
        r.hidden = q !== '' && text(r).toLowerCase().indexOf(q) < 0;
      });
    });
  });

  // Tabs: click, or arrow keys, Home and End on the tab list. Without this script every panel shows.
  doc.querySelectorAll('[data-tabs]').forEach(function (box) {
    var tabs = Array.prototype.slice.call(box.querySelectorAll('[role="tab"]'));
    var panels = tabs.map(function (t) { return doc.getElementById(t.getAttribute('aria-controls')); });
    function show(i) {
      tabs.forEach(function (t, j) {
        var on = i === j;
        t.setAttribute('aria-selected', on ? 'true' : 'false');
        t.tabIndex = on ? 0 : -1;
        if (panels[j]) { panels[j].hidden = !on; }
      });
    }
    tabs.forEach(function (t, i) {
      t.addEventListener('click', function () { show(i); });
      t.addEventListener('keydown', function (e) {
        var k = e.key;
        var n = k === 'ArrowRight' ? (i + 1) % tabs.length : k === 'ArrowLeft' ? (i + tabs.length - 1) % tabs.length : k === 'Home' ? 0 : k === 'End' ? tabs.length - 1 : -1;
        if (n >= 0) { e.preventDefault(); show(n); tabs[n].focus(); }
      });
    });
    box.classList.add('enhanced');
    show(0);
  });

  // Copy buttons: copy the text of the element the button names.
  doc.querySelectorAll('button[data-copy]').forEach(function (b) {
    b.addEventListener('click', function () {
      var target = doc.querySelector(b.getAttribute('data-copy'));
      if (!target) { return; }
      var ta = doc.createElement('textarea');
      ta.value = target.textContent;
      ta.setAttribute('readonly', '');
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      doc.body.appendChild(ta);
      ta.select();
      var ok = false;
      try { ok = doc.execCommand('copy'); } catch (err) { ok = false; }
      doc.body.removeChild(ta);
      var label = b.textContent;
      b.textContent = ok ? 'Copied' : 'Select and copy';
      setTimeout(function () { b.textContent = label; }, 1500);
    });
  });
  // Slide decks: one slide at a time, by keys, buttons or swipe. Without this script every slide is shown in turn,
  // and the notes are shown under each. Slide links are in-page anchors; the page is not navigated.
  doc.querySelectorAll('[data-deck]').forEach(function (deck) {
    var slides = Array.prototype.slice.call(deck.querySelectorAll('.slide'));
    var count = deck.querySelector('[data-deck-count]');
    var panel = deck.querySelector('[data-deck-index-panel]');
    var notesBtn = deck.querySelector('[data-deck-notes]');
    var cur = 0;
    function show(i) {
      cur = Math.max(0, Math.min(slides.length - 1, i));
      slides.forEach(function (s, j) { s.hidden = j !== cur; });
      if (count) { count.textContent = (cur + 1) + ' / ' + slides.length; }
      if (panel) { panel.hidden = true; }
      slides[cur].scrollTop = 0;
    }
    function toggleNotes() {
      var on = deck.classList.toggle('notes-on');
      if (notesBtn) { notesBtn.setAttribute('aria-pressed', on ? 'true' : 'false'); }
    }
    function toggleIndex() { if (panel) { panel.hidden = !panel.hidden; } }
    function wire(selector, fn) {
      deck.querySelectorAll(selector).forEach(function (b) { b.addEventListener('click', fn); });
    }
    wire('[data-deck-prev]', function () { show(cur - 1); });
    wire('[data-deck-next]', function () { show(cur + 1); });
    wire('[data-deck-index]', toggleIndex);
    wire('[data-deck-notes]', toggleNotes);
    if (panel) {
      Array.prototype.forEach.call(panel.querySelectorAll('a'), function (a, i) {
        a.addEventListener('click', function (e) { e.preventDefault(); show(i); });
      });
    }
    doc.addEventListener('keydown', function (e) {
      if (e.ctrlKey || e.metaKey || e.altKey) { return; }
      var t = e.target;
      var k = e.key;
      if (t && t.closest && t.closest('input, textarea, select, [role="tablist"]')) { return; }
      if ((k === ' ' || k === 'Enter') && t && t.closest && t.closest('button, a')) { return; }
      if (k === 'ArrowRight' || k === 'PageDown' || k === ' ') { e.preventDefault(); show(cur + 1); }
      else if (k === 'ArrowLeft' || k === 'PageUp' || k === 'Backspace') { e.preventDefault(); show(cur - 1); }
      else if (k === 'Home') { show(0); }
      else if (k === 'End') { show(slides.length - 1); }
      else if (k === 'n' || k === 'N') { toggleNotes(); }
      else if (k === 'i' || k === 'I') { toggleIndex(); }
      else if (k === 'Escape' && panel && !panel.hidden) { panel.hidden = true; }
    });
    var x0 = null;
    deck.addEventListener('touchstart', function (e) { x0 = e.changedTouches[0].clientX; });
    deck.addEventListener('touchend', function (e) {
      if (x0 === null) { return; }
      var dx = e.changedTouches[0].clientX - x0;
      x0 = null;
      if (Math.abs(dx) > 50) { show(dx < 0 ? cur + 1 : cur - 1); }
    });
    deck.classList.add('enhanced');
    show(0);
  });
  // Charts in a page: a tooltip for whatever is pointed at, legend items that hide and show their series, and a
  // dragged region that is magnified (double-click to go back). Without this script the charts are still complete.
  doc.querySelectorAll('figure.chart').forEach(function (fig) {
    var svg = fig.querySelector('svg');
    if (!svg) { return; }
    var tip = doc.createElement('div');
    tip.className = 'tip';
    tip.hidden = true;
    fig.appendChild(tip);
    var from = null;
    function say(e) {
      var t = e.target && e.target.closest ? e.target.closest('[data-v]') : null;
      if (from || !t || e.target.closest('[data-t]')) { tip.hidden = true; return; }
      tip.textContent = t.getAttribute('data-v');
      tip.hidden = false;
      var r = fig.getBoundingClientRect();
      var x = e.clientX - r.left + 14;
      if (x + tip.offsetWidth > r.width) { x = e.clientX - r.left - tip.offsetWidth - 14; }
      tip.style.left = Math.max(0, x) + 'px';
      tip.style.top = Math.max(0, e.clientY - r.top - tip.offsetHeight - 10) + 'px';
    }
    svg.addEventListener('pointermove', say);
    svg.addEventListener('pointerleave', function () { tip.hidden = true; });

    var off = {};
    svg.querySelectorAll('[data-t]').forEach(function (item) {
      function flip() {
        var j = item.getAttribute('data-t');
        off[j] = !off[j];
        item.setAttribute('aria-pressed', off[j] ? 'false' : 'true');
        item.classList.toggle('off', !!off[j]);
        svg.querySelectorAll('[data-s="' + j + '"]').forEach(function (m) { m.classList.toggle('hid', !!off[j]); });
        tip.hidden = true;
      }
      item.addEventListener('click', flip);
      item.addEventListener('keydown', function (e) {
        if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); flip(); }
      });
    });

    var brush = svg.querySelector('.brush');
    var base = svg.getAttribute('viewBox');
    if (!brush || !base) { return; }
    var box = base.split(' ').map(Number);
    function at(e) {
      var m = svg.getScreenCTM();
      if (!m) { return null; }
      var p = svg.createSVGPoint();
      p.x = e.clientX;
      p.y = e.clientY;
      var q = p.matrixTransform(m.inverse());
      return [q.x, q.y];
    }
    svg.addEventListener('pointerdown', function (e) {
      if (e.button !== 0 || e.target.closest('[data-t]')) { return; }
      from = at(e);
    });
    svg.addEventListener('pointermove', function (e) {
      if (!from) { return; }
      var to = at(e);
      if (!to) { return; }
      brush.setAttribute('x', Math.min(from[0], to[0]));
      brush.setAttribute('y', Math.min(from[1], to[1]));
      brush.setAttribute('width', Math.abs(to[0] - from[0]));
      brush.setAttribute('height', Math.abs(to[1] - from[1]));
      brush.removeAttribute('hidden');
    });
    svg.addEventListener('pointerup', function (e) {
      if (!from) { return; }
      var a = from;
      var to = at(e);
      from = null;
      brush.setAttribute('hidden', '');
      if (!to) { return; }
      var w = Math.abs(to[0] - a[0]);
      var h = Math.abs(to[1] - a[1]);
      if (w < 24 || h < 24) { return; }
      var cx = (a[0] + to[0]) / 2;
      var cy = (a[1] + to[1]) / 2;
      var ratio = box[2] / box[3];
      if (w / h > ratio) { h = w / ratio; } else { w = h * ratio; }
      svg.setAttribute('viewBox', (cx - w / 2) + ' ' + (cy - h / 2) + ' ' + w + ' ' + h);
      fig.classList.add('zoomed');
    });
    svg.addEventListener('dblclick', function () {
      svg.setAttribute('viewBox', base);
      fig.classList.remove('zoomed');
    });
  });
})();
