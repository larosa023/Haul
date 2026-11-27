import { test } from 'node:test';
import assert from 'node:assert/strict';
import { bytes, eta, matchesFilter, matchesSearch, linkKind, urlName, percent, expandPattern, batchLinks, clock } from '../src/lib/format.js';

test('sizes and times', () => {
  assert.equal(bytes(0), '0 B');
  assert.equal(bytes(1536), '1.5 KB');
  assert.equal(bytes(5 * 1024 ** 3), '5.0 GB');
  assert.equal(eta(-1), '');
  assert.equal(eta(30), '30s');
  assert.equal(eta(3700), '1h 1m');
  assert.equal(percent(5, 10), 50);
});

test('filters', () => {
  assert.ok(matchesFilter({ status: 'queued' }, 'downloading'));
  assert.ok(matchesFilter({ status: 'seeding' }, 'completed'));
  assert.ok(!matchesFilter({ status: 'paused' }, 'downloading'));
  assert.ok(matchesFilter({ status: 'error' }, 'errors'));
  assert.ok(matchesSearch({ name: 'Ubuntu 24.04 ISO', category: 'Software' }, 'ubuntu iso'));
  assert.ok(!matchesSearch({ name: 'Ubuntu', category: '' }, 'fedora'));
});

test('links', () => {
  assert.equal(linkKind('magnet:?xt=urn:btih:abc'), 'torrent');
  assert.equal(linkKind('https://x.org/a.torrent'), 'torrent');
  assert.equal(linkKind('https://x.org/a.zip'), 'http');
  assert.equal(linkKind('ftp://x.org/a.iso'), 'http');
  assert.equal(linkKind('gopher://x'), null);
  assert.equal(linkKind('https://www.youtube.com/watch?v=abc'), 'media');
  assert.equal(linkKind('https://youtube.com/'), 'http');
  assert.equal(linkKind('https://cdn.x/live/master.m3u8'), 'media');
  assert.equal(linkKind('C:\\Users\\a\\b.torrent'), 'torrent');
  assert.equal(linkKind('hello'), null);
  assert.equal(urlName('https://x.org/dl/file%20a.zip?x=1'), 'file a.zip');
  assert.equal(urlName('magnet:?xt=urn:btih:abc&dn=Big+Buck'), 'Big Buck');
});

test('batch patterns', () => {
  assert.deepEqual(expandPattern('f[1-3].zip'), ['f1.zip', 'f2.zip', 'f3.zip']);
  assert.deepEqual(expandPattern('f[008-010].zip'), ['f008.zip', 'f009.zip', 'f010.zip']);
  assert.deepEqual(expandPattern('p[a-c]'), ['pa', 'pb', 'pc']);
  assert.deepEqual(expandPattern('i[0-4:2]'), ['i0', 'i2', 'i4']);
  assert.equal(expandPattern('d[1-2]/[a-b]').length, 4);
  assert.equal(expandPattern('none'), null);
  assert.equal(expandPattern('x[1-99999]', 100).length, 100);
  assert.equal(batchLinks('https://a.org/f[01-12].bin\nhello\nhttps://b.org/x.zip https://b.org/x.zip').length, 13);
  assert.equal(clock(3725), '1:02:05');
});
