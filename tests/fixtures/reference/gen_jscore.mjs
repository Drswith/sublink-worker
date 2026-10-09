import { base64ToBinary, decodeBase64, encodeBase64 } from './src/utils.js';
const show = (n) => Object.is(n, -0) ? '-0' : String(n);
const numInputs = ['', ' ', '0', '-0', '1', '+1', '-1', '1.5', '.5', '5.', '1e3', '1E-7', '1e21', '123456789012345678901234', '0x1F', '0X1f', '-0x1F', '0o17', '0b101', '0b', '0x', 'Infinity', '-Infinity', '+Infinity', 'infinity', 'NaN', 'abc', '12abc', '  42  ', ' 42　', '﻿7', '1_000', '010', '00.5', '1.2.3', '9007199254740993', '0.1', '0.000001', '0.0000001', '1e-7', '100', '1e100', '-1e-100', '5e-324', '1.7976931348623157e308', '2e308', '3.14159', '443', '20000-30000', '30s', '  -12.5e2xyz', '0.30000000000000004', '123.456e-2', '\n1\n', '1 2', '- 1', '0x1g', '1e', '1e+', '-.5', '+.5e1', '.e1'];
const out = [];
for (const s of numInputs) out.push({ fn: 'num', input: s, n: show(Number(s)), pf: show(parseFloat(s)), pi: show(parseInt(s)) });
const uriInputs = ['abc', '%20', '%2', '%', '%zz', '%E4%BD%A0%E5%A5%BD', '%E4%BD', '%C0%AF', '%ED%A0%80', '%F4%90%80%80', '%F0%9F%98%80', '%80', 'a%2Fb%3Fc%23d', 'a+b', '%41%42', '100%', '%%41', '%E4%BD%A0%', '中文%20x', '%3B%2F%3F%3A%40%26%3D%2B%24%2C%23', '%FF', '%e4%bd%a0'];
for (const s of uriInputs) {
  let c, u;
  try { c = decodeURIComponent(s); } catch (e) { c = 'ERR:' + e.name + ':' + e.message; }
  try { u = decodeURI(s); } catch (e) { u = 'ERR:' + e.name + ':' + e.message; }
  out.push({ fn: 'uri', input: s, component: c, uri: u });
}
const encInputs = ['abc', 'a b', '中文', 'a/b?c#d', "!~*'()", '😀', ';/?:@&=+$,#', '%41', 'http://x.com/路径?q=值'];
for (const s of encInputs) out.push({ fn: 'enc', input: s, component: encodeURIComponent(s), uri: encodeURI(s) });
const b64Inputs = ['YWVzLTEyOC1nY206dGVzdA', 'YWVzLTEyOC1nY206dGVzdA==', 'YWVz LTEy', 'YW-_', 'SGVsbG8gV29ybGQh', '5L2g5aW9', '77u/5L2g', '!!!!', 'Y', 'YW', 'YWE', 'YWFh', 'Y=Y=', '中文', 'eyJhIjoxfQ', 'ZmFsc2U\n', '/w==', '//79', 'gICA', '4pyTIMOgIGxhIG1vZGU='];
for (const s of b64Inputs) out.push({ fn: 'b64', input: s, binary: base64ToBinary(s), decoded: decodeBase64(s) });
const encB64 = ['', 'a', 'ab', 'abc', 'abcd', '你好', '😀', 'ss://YWVz@x:1#名字\nvmess://e30='];
for (const s of encB64) out.push({ fn: 'b64enc', input: s, output: encodeBase64(s) });
const jsonInputs = ['', ' ', 'a', 'abc', '{', '{"a":1', '{"a" 1}', '{a:1}', '[1,2', '[1 2]', '"abc', '"a\\x"', '"\\u12G4"', '-', '1.', '1e', '01', '{"a":1}x', 'undefined', 'NaN', 'Infinity', '[object Object]', 'null x', 'tru', 'nul', 'fals', 'falsy', '{"a":1,}', '[1,]', '"\u0001"', 'this is a long string with error', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '{"a": 1, "b": 2, "c": x, "d": 4, "e": 5, "f": 6}', '{"abcdefghijklmnopqrst": tru}', 'proxies:\n  - name: a', '1 2', '"a" "b"', '{"a":1 "b":2}', '{"a":1}}', '[]]', '\n\n  x', '{\n"a":\n}', '-x', '1.x', '1ex', '{"a":-}', '"\\', '[', ']', '{}', '0x10', '+1', '.5', '"\t"', ' ', '{"a":"b\n"}', '{\r\n"a":\r\n x}', '{\r"a": x}', '中文', '{"中文": 1, "x": y}', '{"a":1,"b":[1,2,{"c":"d"}],"1":true,"0":null,"e":1.5e3,"f":-0,"g":"\\u00e9\\n\\"\\/"}', '[1e400, -1e400, 1e-400, 0.1, 123456789012345678]', '{"b":1,"a":2,"b":3}', '{"__proto__": 1}', '"\\ud83d\\ude00"', '  [ ]  ', '{"a":{}}', '"emoji 😀"', 'vmessgarbage{"v":"2"}', '﻿{}'];
for (const s of jsonInputs) {
  let r;
  try { r = { ok: JSON.stringify(JSON.parse(s)) }; } catch (e) { r = { err: e.name + ':' + e.message }; }
  out.push({ fn: 'json', input: s, ...r });
}
const qsInputs = ['a=1&b=2', '?a=1&a=2', 'a=%E4%B8%AD%FF', 'sni=中%FF', 'x=%zz中', 'x=中%', 'x=%E4%B8', 'a+b=c+d', 'a=%2B+%20', '&&a=1&&', '=x', 'a', 'a=', 'a==b', 'a=b=c', '%61=1', '%61%FF中=v', 'k=%ED%A0%80', 'k=%F0%9F%98%80', 'k=😀%FF', 'k=%%41', 'k=%4', 'k=%41%4', 'k=%G1%41', 'k=a%2', 'k=%E4%B8%AD%25FF', 'k=a%FFb中c', 'k=%C3%28', 'k=%u4E2D%41', 'k=%0', 'k=%%', 'k=中%E4', 'k=éé%FF', 'k=%41%zz%FFé', 'k=%41%4é%FF', 'path=%2Fws%3Fed%3D2048&host=a.com', ''];
for (const s of qsInputs) out.push({ fn: 'qs', input: s, entries: JSON.stringify([...new URLSearchParams(s)]) });
const trimInputs = [' a ', ' a　', '﻿a ', '\u0085a\u0085', '\t\n\r\v\fa', '​a​', '᠎a'];
for (const s of trimInputs) out.push({ fn: 'trim', input: s, output: s.trim() });
console.log(JSON.stringify(out));
