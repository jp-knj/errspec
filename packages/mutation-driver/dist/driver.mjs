// driver.mjs
import { readFileSync as readFileSync3, writeFileSync, realpathSync as realpathSync3, existsSync as existsSync3, globSync, statSync } from "node:fs";
import { basename, dirname as dirname3, resolve as resolve3, relative as relative2, join as join3, isAbsolute as isAbsolute2, matchesGlob } from "node:path";
import { pathToFileURL as pathToFileURL3 } from "node:url";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { createInterface } from "node:readline";
import { format } from "node:util";
import { serialize } from "node:v8";

// node_modules/@jridgewell/sourcemap-codec/dist/sourcemap-codec.mjs
var comma = 44;
var semicolon = 59;
var chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
var intToChar = new Uint8Array(64);
var charToInt = new Uint8Array(128);
for (let i = 0;i < chars.length; i++) {
  const c = chars.charCodeAt(i);
  intToChar[i] = c;
  charToInt[c] = i;
}
function encodeInteger(builder, num) {
  do {
    let clamped = num & 31;
    num >>>= 5;
    if (num > 0)
      clamped |= 32;
    builder.write(intToChar[clamped]);
  } while (num > 0);
}
function encodeSign(num) {
  return num < 0 ? -num << 1 | 1 : num << 1;
}
var bufLength = 1024 * 16;
var td = typeof TextDecoder !== "undefined" ? /* @__PURE__ */ new TextDecoder : typeof Buffer !== "undefined" ? {
  decode(buf) {
    const out = Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength);
    return out.toString();
  }
} : {
  decode(buf) {
    let out = "";
    for (let i = 0;i < buf.length; i++) {
      out += String.fromCharCode(buf[i]);
    }
    return out;
  }
};
var StringWriter = class {
  constructor() {
    this.pos = 0;
    this.out = "";
    this.buffer = new Uint8Array(bufLength);
  }
  write(v) {
    const { buffer } = this;
    buffer[this.pos++] = v;
    if (this.pos === bufLength) {
      this.out += td.decode(buffer);
      this.pos = 0;
    }
  }
  flush() {
    const { buffer, out, pos } = this;
    return pos > 0 ? out + td.decode(buffer.subarray(0, pos)) : out;
  }
};
function encodeRangeMappings(decoded) {
  if (decoded.length === 0)
    return "";
  const writer = new StringWriter;
  for (let i = 0;i < decoded.length; i++) {
    const indices = decoded[i];
    if (i > 0)
      writer.write(semicolon);
    let index = 0;
    for (let j = 0;j < indices.length; j++) {
      const offset = indices[j];
      encodeInteger(writer, offset - index);
      index = offset;
    }
  }
  return writer.flush();
}
function encode(decoded) {
  const writer = new StringWriter;
  let sourcesIndex = 0;
  let sourceLine = 0;
  let sourceColumn = 0;
  let namesIndex = 0;
  for (let i = 0;i < decoded.length; i++) {
    const line = decoded[i];
    if (i > 0)
      writer.write(semicolon);
    if (line.length === 0)
      continue;
    let genColumn = 0;
    for (let j = 0;j < line.length; j++) {
      const segment = line[j];
      if (j > 0)
        writer.write(comma);
      encodeInteger(writer, encodeSign(segment[0] - genColumn));
      genColumn = segment[0];
      if (segment.length === 1)
        continue;
      encodeInteger(writer, encodeSign(segment[1] - sourcesIndex));
      encodeInteger(writer, encodeSign(segment[2] - sourceLine));
      encodeInteger(writer, encodeSign(segment[3] - sourceColumn));
      sourcesIndex = segment[1];
      sourceLine = segment[2];
      sourceColumn = segment[3];
      if (segment.length === 4)
        continue;
      encodeInteger(writer, encodeSign(segment[4] - namesIndex));
      namesIndex = segment[4];
    }
  }
  return writer.flush();
}

// node_modules/magic-string/dist/index.mjs
var BitSet = class BitSet {
  constructor(arg) {
    this.bits = arg instanceof BitSet ? arg.bits.slice() : [];
  }
  add(n) {
    this.bits[n >> 5] |= 1 << (n & 31);
  }
  has(n) {
    return !!(this.bits[n >> 5] & 1 << (n & 31));
  }
  forEach(fn) {
    const bits = this.bits;
    for (let w = 0;w < bits.length; w++) {
      let word = bits[w];
      while (word) {
        const lowest = word & -word;
        fn((w << 5) + 31 - Math.clz32(lowest));
        word ^= lowest;
      }
    }
  }
};
var Chunk = class Chunk {
  constructor(start, end, content) {
    this.start = start;
    this.end = end;
    this.original = content;
    this.intro = "";
    this.outro = "";
    this.content = content;
    this.storeName = false;
    this.edited = false;
    this.replacesUntil = -1;
    this.replacedFrom = -1;
    this.previous = null;
    this.next = null;
  }
  appendLeft(content) {
    this.outro += content;
  }
  appendRight(content) {
    this.intro = this.intro + content;
  }
  clone() {
    const chunk = new Chunk(this.start, this.end, this.original);
    chunk.intro = this.intro;
    chunk.outro = this.outro;
    chunk.content = this.content;
    chunk.storeName = this.storeName;
    chunk.edited = this.edited;
    chunk.replacesUntil = this.replacesUntil;
    chunk.replacedFrom = this.replacedFrom;
    return chunk;
  }
  contains(index) {
    return this.start < index && index < this.end;
  }
  eachNext(fn) {
    fn(this);
    let chunk = this.next;
    while (chunk) {
      fn(chunk);
      chunk = chunk.next;
    }
  }
  eachPrevious(fn) {
    fn(this);
    let chunk = this.previous;
    while (chunk) {
      fn(chunk);
      chunk = chunk.previous;
    }
  }
  edit(content, storeName, contentOnly) {
    this.content = content;
    if (!contentOnly) {
      this.intro = "";
      this.outro = "";
    }
    this.storeName = storeName;
    this.edited = true;
    return this;
  }
  prependLeft(content) {
    this.outro = content + this.outro;
  }
  prependRight(content) {
    this.intro = content + this.intro;
  }
  reset() {
    this.intro = "";
    this.outro = "";
    this.replacesUntil = -1;
    this.replacedFrom = -1;
    if (this.edited) {
      this.content = this.original;
      this.storeName = false;
      this.edited = false;
    }
  }
  split(index) {
    const sliceIndex = index - this.start;
    const originalBefore = this.original.slice(0, sliceIndex);
    const originalAfter = this.original.slice(sliceIndex);
    this.original = originalBefore;
    const newChunk = new Chunk(index, this.end, originalAfter);
    newChunk.outro = this.outro;
    this.outro = "";
    this.end = index;
    if (this.edited) {
      newChunk.edit("", false, true);
      newChunk.replacedFrom = this.replacedFrom;
    } else
      this.content = originalBefore;
    newChunk.next = this.next;
    if (newChunk.next)
      newChunk.next.previous = newChunk;
    newChunk.previous = this;
    this.next = newChunk;
    return newChunk;
  }
  toString() {
    return this.intro + this.content + this.outro;
  }
  trimEnd(rx) {
    this.outro = this.outro.replace(rx, "");
    if (this.outro.length)
      return true;
    const trimmed = this.content.replace(rx, "");
    if (trimmed.length) {
      if (trimmed !== this.content) {
        if (this.edited)
          this.edit(trimmed, this.storeName, true);
        else
          this.split(this.start + trimmed.length).edit("", undefined, true);
      }
      return true;
    } else {
      this.edit("", undefined, true);
      this.intro = this.intro.replace(rx, "");
      if (this.intro.length)
        return true;
    }
  }
  trimStart(rx) {
    this.intro = this.intro.replace(rx, "");
    if (this.intro.length)
      return true;
    const trimmed = this.content.replace(rx, "");
    if (trimmed.length) {
      if (trimmed !== this.content) {
        if (this.edited)
          this.edit(trimmed, this.storeName, true);
        else {
          this.split(this.end - trimmed.length);
          this.edit("", undefined, true);
        }
      }
      return true;
    } else {
      this.edit("", undefined, true);
      this.outro = this.outro.replace(rx, "");
      if (this.outro.length)
        return true;
    }
  }
};
var MagicStringError = class extends Error {
  name = "MagicStringError";
  constructor(message, options) {
    super(`[MagicString] ${message}`, options);
  }
};
function getBtoa() {
  if (typeof globalThis !== "undefined" && typeof globalThis.btoa === "function")
    return (str) => globalThis.btoa(unescape(encodeURIComponent(str)));
  const buffer = globalThis["Buffer"];
  if (buffer)
    return (str) => buffer.from(str, "utf-8").toString("base64");
  return () => {
    throw new MagicStringError("unsupported environment: `btoa` or `Buffer` is required");
  };
}
var btoa = /* @__PURE__ */ getBtoa();
var SourceMap = class {
  constructor(properties) {
    this.version = 3;
    this.file = properties.file;
    this.sources = properties.sources;
    this.sourcesContent = properties.sourcesContent;
    this.names = properties.names;
    this.mappings = typeof properties.mappings === "string" ? properties.mappings : encode(properties.mappings);
    if (typeof properties.x_google_ignoreList !== "undefined")
      this.x_google_ignoreList = properties.x_google_ignoreList;
    if (typeof properties.debugId !== "undefined")
      this.debugId = properties.debugId;
    if (typeof properties.rangeMappings !== "undefined") {
      let shouldOutputRangeMapping = false;
      for (const line of properties.rangeMappings)
        if (line.length !== 0) {
          shouldOutputRangeMapping = true;
          break;
        }
      if (shouldOutputRangeMapping)
        this.rangeMappings = encodeRangeMappings(properties.rangeMappings);
    }
  }
  toString() {
    return JSON.stringify(this);
  }
  toUrl() {
    return `data:application/json;charset=utf-8;base64,${btoa(this.toString())}`;
  }
};
function getLocator(source) {
  const lineOffsets = [0];
  for (let i = source.indexOf(`
`);i !== -1; i = source.indexOf(`
`, i + 1))
    lineOffsets.push(i + 1);
  return function locate(index) {
    let i = 0;
    let j = lineOffsets.length;
    while (i < j) {
      const m = i + j >> 1;
      if (index < lineOffsets[m])
        j = m;
      else
        i = m + 1;
    }
    const line = i - 1;
    return {
      line,
      column: index - lineOffsets[line]
    };
  };
}
function getRelativePath(from, to) {
  const fromParts = from.split(/[/\\]/);
  const toParts = to.split(/[/\\]/);
  fromParts.pop();
  while (fromParts[0] === toParts[0]) {
    fromParts.shift();
    toParts.shift();
  }
  if (fromParts.length) {
    let i = fromParts.length;
    while (i--)
      fromParts[i] = "..";
  }
  return fromParts.concat(toParts).join("/");
}
function guessIndent(code) {
  const lines = code.split(`
`);
  const tabbed = lines.filter((line) => /^\t+/.test(line));
  const spaced = lines.filter((line) => /^ {2,}/.test(line));
  if (tabbed.length === 0 && spaced.length === 0)
    return null;
  if (tabbed.length >= spaced.length)
    return "\t";
  const min = spaced.reduce((previous, current) => {
    const numSpaces = /^ +/.exec(current)[0].length;
    return Math.min(numSpaces, previous);
  }, Infinity);
  return " ".repeat(min);
}
var toString = Object.prototype.toString;
function isObject(thing) {
  return toString.call(thing) === "[object Object]";
}
var BASE64_CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
var intToChar2 = /* @__PURE__ */ (() => {
  const chars = /* @__PURE__ */ new Uint8Array(64);
  for (let i = 0;i < 64; i++)
    chars[i] = BASE64_CHARS.charCodeAt(i);
  return chars;
})();
var COMMA = 44;
var SEMICOLON = 59;
var BUFFER_SIZE = 16384;
var FLUSH_THRESHOLD = 16348;
var scratch = /* @__PURE__ */ new Uint8Array(BUFFER_SIZE);
function writeVlq(pos, num) {
  num = num < 0 ? -num << 1 | 1 : num << 1;
  do {
    let clamped = num & 31;
    num >>>= 5;
    if (num > 0)
      clamped |= 32;
    scratch[pos++] = intToChar2[clamped];
  } while (num > 0);
  return pos;
}
var decoder = /* @__PURE__ */ new TextDecoder;
var MappingsEncoder = class {
  constructor() {
    this.out = "";
    this.pos = 0;
    this.lines = [];
    this.buffered = 0;
    this.needsComma = false;
    this.prevGenColumn = 0;
    this.prevSourceIndex = 0;
    this.prevSourceLine = 0;
    this.prevSourceColumn = 0;
    this.prevNameIndex = 0;
  }
  endLine(segments) {
    this.lines.push(segments);
    this.buffered += segments.length + 1;
    if (this.buffered >= 4096)
      this.drain(null);
  }
  segments(segments) {
    this.drain(segments);
  }
  finish(segments) {
    this.drain(segments);
    this.flush();
    return this.out;
  }
  drain(trailing) {
    const lines = this.lines;
    const lineCount = lines.length;
    for (let l = 0;l <= lineCount; l++) {
      const line = l < lineCount ? lines[l] : trailing;
      if (line === null)
        break;
      for (let i = 0;i < line.length; i++) {
        if (this.pos > FLUSH_THRESHOLD)
          this.flush();
        const segment = line[i];
        if (this.needsComma)
          scratch[this.pos++] = COMMA;
        this.needsComma = true;
        this.pos = writeVlq(this.pos, segment[0] - this.prevGenColumn);
        this.prevGenColumn = segment[0];
        this.pos = writeVlq(this.pos, segment[1] - this.prevSourceIndex);
        this.prevSourceIndex = segment[1];
        this.pos = writeVlq(this.pos, segment[2] - this.prevSourceLine);
        this.prevSourceLine = segment[2];
        this.pos = writeVlq(this.pos, segment[3] - this.prevSourceColumn);
        this.prevSourceColumn = segment[3];
        if (segment.length === 5) {
          this.pos = writeVlq(this.pos, segment[4] - this.prevNameIndex);
          this.prevNameIndex = segment[4];
        }
      }
      if (l < lineCount) {
        if (this.pos > FLUSH_THRESHOLD)
          this.flush();
        scratch[this.pos++] = SEMICOLON;
        this.needsComma = false;
        this.prevGenColumn = 0;
      }
    }
    lines.length = 0;
    this.buffered = 0;
    this.flush();
  }
  flush() {
    this.out += decoder.decode(scratch.subarray(0, this.pos));
    this.pos = 0;
  }
};
var NEWLINE_CHAR$1 = 10;
function isWordCode(code) {
  return code >= 97 && code <= 122 || code >= 65 && code <= 90 || code >= 48 && code <= 57 || code === 95;
}
var Mappings = class {
  constructor(hires, encoder = null) {
    this.hires = hires;
    this.generatedCodeLine = 0;
    this.generatedCodeColumn = 0;
    this.raw = [];
    this.rawSegments = this.raw[this.generatedCodeLine] = [];
    this.rawRangeMappingsIndices = [];
    this.rawRangeMappings = hires === "experimental-range" ? [this.rawRangeMappingsIndices] : undefined;
    this.encoder = encoder;
  }
  nextLine() {
    if (this.encoder === null) {
      this.generatedCodeLine += 1;
      this.raw[this.generatedCodeLine] = this.rawSegments = [];
    } else {
      this.encoder.endLine(this.rawSegments);
      this.rawSegments = [];
      this.generatedCodeLine += 1;
    }
    this.generatedCodeColumn = 0;
    if (this.rawRangeMappings)
      this.rawRangeMappings[this.generatedCodeLine] = this.rawRangeMappingsIndices = [];
  }
  addMagicString(magicString, sourceIndex, nameIndex) {
    const { original } = magicString;
    const locate = getLocator(original);
    if (magicString.intro)
      this.advance(magicString.intro);
    magicString.firstChunk.eachNext((chunk) => {
      if (chunk.intro.length)
        this.advance(chunk.intro);
      if (sourceIndex === -1)
        this.advance(chunk.content);
      else if (chunk.edited) {
        const name = chunk.storeName ? nameIndex.get(original.slice(chunk.start, chunk.replacesUntil)) : undefined;
        this.addEdit(sourceIndex, chunk.content, locate(chunk.start), name ?? -1);
      } else
        this.addUneditedChunk(sourceIndex, chunk, original, locate(chunk.start), magicString.sourcemapLocations);
      if (chunk.outro.length)
        this.advance(chunk.outro);
    });
    if (magicString.outro)
      this.advance(magicString.outro);
  }
  addEdit(sourceIndex, content, loc, nameIndex) {
    if (content.length) {
      const contentLengthMinusOne = content.length - 1;
      let contentLineEnd = content.indexOf(`
`, 0);
      let previousContentLineEnd = -1;
      while (contentLineEnd >= 0 && contentLengthMinusOne > contentLineEnd) {
        const segment = [
          this.generatedCodeColumn,
          sourceIndex,
          loc.line,
          loc.column
        ];
        if (nameIndex >= 0)
          segment.push(nameIndex);
        this.rawSegments.push(segment);
        this.nextLine();
        previousContentLineEnd = contentLineEnd;
        contentLineEnd = content.indexOf(`
`, contentLineEnd + 1);
      }
      const segment = [
        this.generatedCodeColumn,
        sourceIndex,
        loc.line,
        loc.column
      ];
      if (nameIndex >= 0)
        segment.push(nameIndex);
      this.rawSegments.push(segment);
      this.advance(content.slice(previousContentLineEnd + 1));
    }
  }
  addUneditedChunk(sourceIndex, chunk, original, loc, sourcemapLocations) {
    const end = chunk.end;
    let i = chunk.start;
    if (this.hires) {
      const boundary = this.hires === "boundary";
      const experimentalRange = this.hires === "experimental-range";
      const encoder = experimentalRange ? null : this.encoder;
      let charInHiresBoundary = false;
      while (i < end) {
        if (encoder !== null && this.rawSegments.length >= 4096) {
          encoder.segments(this.rawSegments);
          this.rawSegments.length = 0;
        }
        if (experimentalRange && i + 1 >= end)
          this.rawSegments.push([
            this.generatedCodeColumn,
            sourceIndex,
            loc.line,
            loc.column
          ]);
        const code = original.charCodeAt(i);
        if (code === NEWLINE_CHAR$1) {
          loc.line += 1;
          loc.column = 0;
          this.nextLine();
          charInHiresBoundary = false;
        } else {
          if (boundary) {
            if (isWordCode(code)) {
              if (!charInHiresBoundary) {
                this.rawSegments.push([
                  this.generatedCodeColumn,
                  sourceIndex,
                  loc.line,
                  loc.column
                ]);
                charInHiresBoundary = true;
              }
            } else {
              this.rawSegments.push([
                this.generatedCodeColumn,
                sourceIndex,
                loc.line,
                loc.column
              ]);
              charInHiresBoundary = false;
            }
          } else if (experimentalRange) {
            if (i === chunk.start) {
              this.rawRangeMappingsIndices.push(this.rawSegments.length);
              this.rawSegments.push([
                this.generatedCodeColumn,
                sourceIndex,
                loc.line,
                loc.column
              ]);
            }
          } else
            this.rawSegments.push([
              this.generatedCodeColumn,
              sourceIndex,
              loc.line,
              loc.column
            ]);
          loc.column += 1;
          this.generatedCodeColumn += 1;
        }
        i += 1;
      }
    } else {
      const bits = sourcemapLocations.bits;
      while (i < end) {
        let newline = original.indexOf(`
`, i);
        if (newline === -1 || newline > end)
          newline = end;
        if (newline > i) {
          this.rawSegments.push([
            this.generatedCodeColumn,
            sourceIndex,
            loc.line,
            loc.column
          ]);
          for (let w = i + 1 >> 5, last = newline - 1 >> 5;w <= last; w++) {
            let word = bits[w];
            if (!word)
              continue;
            const base = w << 5;
            while (word) {
              const lowest = word & -word;
              const index = base + 31 - Math.clz32(lowest);
              if (index > i && index < newline) {
                const offset = index - i;
                this.rawSegments.push([
                  this.generatedCodeColumn + offset,
                  sourceIndex,
                  loc.line,
                  loc.column + offset
                ]);
              }
              word ^= lowest;
            }
          }
          loc.column += newline - i;
          this.generatedCodeColumn += newline - i;
        }
        if (newline === end)
          break;
        loc.line += 1;
        loc.column = 0;
        this.nextLine();
        i = newline + 1;
      }
    }
  }
  advance(str) {
    if (!str)
      return;
    const lastNewline = str.lastIndexOf(`
`);
    for (let i = str.indexOf(`
`);i !== -1; i = str.indexOf(`
`, i + 1))
      this.nextLine();
    this.generatedCodeColumn += str.length - lastNewline - 1;
  }
};
var n = `
`;
var NEWLINE_CHAR = 10;
var CR_CHAR = 13;
var warned = {
  insertLeft: false,
  insertRight: false,
  storeName: false
};
function expandReplacement(replacement, matched, position, str, captures, namedCaptures) {
  if (!replacement.includes("$"))
    return replacement;
  let result = "";
  let index = 0;
  while (index < replacement.length) {
    const dollar = replacement.indexOf("$", index);
    if (dollar === -1) {
      result += replacement.slice(index);
      break;
    }
    result += replacement.slice(index, dollar);
    const char = replacement[dollar + 1];
    let expansion = "$";
    let consumed = 1;
    if (char === "$")
      consumed = 2;
    else if (char === "&") {
      expansion = matched;
      consumed = 2;
    } else if (char === "`") {
      expansion = str.slice(0, position);
      consumed = 2;
    } else if (char === "'") {
      expansion = str.slice(position + matched.length);
      consumed = 2;
    } else if (char === "<" && namedCaptures !== undefined) {
      const close = replacement.indexOf(">", dollar + 2);
      if (close !== -1) {
        expansion = namedCaptures[replacement.slice(dollar + 2, close)] ?? "";
        consumed = close + 1 - dollar;
      }
    } else if (char >= "0" && char <= "9") {
      const second = replacement[dollar + 2];
      const double = second >= "0" && second <= "9" ? Number(char + second) : NaN;
      const single = Number(char);
      if (double >= 1 && double <= captures.length) {
        expansion = captures[double - 1] ?? "";
        consumed = 3;
      } else if (single >= 1 && single <= captures.length) {
        expansion = captures[single - 1] ?? "";
        consumed = 2;
      }
    }
    result += expansion;
    index = dollar + consumed;
  }
  return result;
}
var MagicString = class MagicString {
  constructor(string, options = {}) {
    const chunk = new Chunk(0, string.length, string);
    Object.defineProperties(this, {
      original: {
        writable: true,
        value: string
      },
      outro: {
        writable: true,
        value: ""
      },
      intro: {
        writable: true,
        value: ""
      },
      firstChunk: {
        writable: true,
        value: chunk
      },
      lastChunk: {
        writable: true,
        value: chunk
      },
      lastSearchedChunk: {
        writable: true,
        value: chunk
      },
      byStart: {
        writable: true,
        value: /* @__PURE__ */ new Map([[0, chunk]])
      },
      byEnd: {
        writable: true,
        value: /* @__PURE__ */ new Map([[string.length, chunk]])
      },
      filename: {
        writable: true,
        value: options.filename
      },
      indentExclusionRanges: {
        writable: true,
        value: options.indentExclusionRanges
      },
      sourcemapLocations: {
        writable: true,
        value: new BitSet
      },
      storedNames: {
        writable: true,
        value: /* @__PURE__ */ new Set
      },
      indentStr: {
        writable: true,
        value: undefined
      },
      ignoreList: {
        writable: true,
        value: options.ignoreList
      },
      offset: {
        writable: true,
        value: options.offset || 0
      },
      hasMovedChunks: {
        writable: true,
        value: false
      }
    });
  }
  addSourcemapLocation(char) {
    this.sourcemapLocations.add(char);
  }
  append(content) {
    if (typeof content !== "string")
      throw new MagicStringError(`content must be a string, got ${typeof content}`);
    this.outro += content;
    return this;
  }
  appendLeft(index, content) {
    index = this._resolveIndex(index);
    if (index < 0 || index > this.original.length)
      throw new MagicStringError(`index ${index} is out of bounds`);
    if (typeof content !== "string")
      throw new MagicStringError(`content must be a string, got ${typeof content}`);
    this._split(index);
    const chunk = this.byEnd.get(index);
    if (chunk)
      chunk.appendLeft(content);
    else
      this.intro += content;
    return this;
  }
  appendRight(index, content) {
    index = this._resolveIndex(index);
    if (index < 0 || index > this.original.length)
      throw new MagicStringError(`index ${index} is out of bounds`);
    if (typeof content !== "string")
      throw new MagicStringError(`content must be a string, got ${typeof content}`);
    this._split(index);
    const chunk = this.byStart.get(index);
    if (chunk)
      chunk.appendRight(content);
    else
      this.outro += content;
    return this;
  }
  clone() {
    const cloned = new MagicString(this.original, {
      filename: this.filename,
      ignoreList: this.ignoreList,
      offset: this.offset
    });
    let originalChunk = this.firstChunk;
    let clonedChunk = cloned.firstChunk = cloned.lastSearchedChunk = originalChunk.clone();
    while (originalChunk) {
      cloned.byStart.set(clonedChunk.start, clonedChunk);
      cloned.byEnd.set(clonedChunk.end, clonedChunk);
      const nextOriginalChunk = originalChunk.next;
      const nextClonedChunk = nextOriginalChunk && nextOriginalChunk.clone();
      if (nextClonedChunk) {
        clonedChunk.next = nextClonedChunk;
        nextClonedChunk.previous = clonedChunk;
        clonedChunk = nextClonedChunk;
      }
      originalChunk = nextOriginalChunk;
    }
    cloned.lastChunk = clonedChunk;
    if (this.indentExclusionRanges)
      cloned.indentExclusionRanges = this.indentExclusionRanges.slice();
    cloned.sourcemapLocations = new BitSet(this.sourcemapLocations);
    cloned.storedNames = new Set(this.storedNames);
    cloned.intro = this.intro;
    cloned.outro = this.outro;
    cloned.hasMovedChunks = this.hasMovedChunks;
    return cloned;
  }
  generateDecodedMap(options) {
    options = options || {};
    const mappings = new Mappings(options.hires);
    const names = this._generateMappings(mappings);
    return {
      ...this._mapProperties(options, names),
      mappings: mappings.raw,
      rangeMappings: mappings.rawRangeMappings
    };
  }
  generateMap(options) {
    options = options || {};
    const encoder = new MappingsEncoder;
    const mappings = new Mappings(options.hires, encoder);
    const names = this._generateMappings(mappings);
    return new SourceMap({
      ...this._mapProperties(options, names),
      mappings: encoder.finish(mappings.rawSegments),
      rangeMappings: mappings.rawRangeMappings
    });
  }
  _generateMappings(mappings) {
    const names = [...this.storedNames];
    mappings.addMagicString(this, 0, new Map(names.map((name, i) => [name, i])));
    return names;
  }
  _mapProperties(options, names) {
    return {
      file: options.file ? options.file.split(/[/\\]/).pop() : undefined,
      sources: [options.source ? getRelativePath(options.file || "", options.source) : options.file || ""],
      sourcesContent: options.includeContent ? [this.original] : undefined,
      names,
      x_google_ignoreList: this.ignoreList ? [0] : undefined
    };
  }
  _ensureindentStr() {
    if (this.indentStr === undefined)
      this.indentStr = guessIndent(this.original);
  }
  _getRawIndentString() {
    this._ensureindentStr();
    return this.indentStr;
  }
  getIndentString() {
    this._ensureindentStr();
    return this.indentStr ?? "\t";
  }
  indent(indentStr, options) {
    const pattern = /^[^\r\n]/gm;
    if (isObject(indentStr)) {
      options = indentStr;
      indentStr = undefined;
    }
    if (indentStr === undefined) {
      this._ensureindentStr();
      indentStr = this.indentStr || "\t";
    }
    if (indentStr === "")
      return this;
    const resolvedIndentStr = indentStr;
    options = options || {};
    const isExcluded = new BitSet;
    if (options.exclude)
      (typeof options.exclude[0] === "number" ? [options.exclude] : options.exclude).forEach((exclusion) => {
        for (let i = exclusion[0];i < exclusion[1]; i += 1)
          isExcluded.add(i);
      });
    let shouldIndentNextCharacter = options.indentStart !== false;
    const indentPiece = (str) => {
      if (str === "")
        return str;
      const indented = str.replace(pattern, (match, offset) => offset > 0 || shouldIndentNextCharacter ? `${resolvedIndentStr}${match}` : match);
      shouldIndentNextCharacter = str[str.length - 1] === `
`;
      return indented;
    };
    this.intro = indentPiece(this.intro);
    let charIndex = 0;
    let chunk = this.firstChunk;
    const indentAt = (index) => {
      shouldIndentNextCharacter = false;
      if (index === chunk.start)
        chunk.appendRight(resolvedIndentStr);
      else {
        this._splitChunk(chunk, index);
        chunk = chunk.next;
        chunk.prependRight(resolvedIndentStr);
      }
    };
    while (chunk) {
      const end = chunk.end;
      if (!isExcluded.has(chunk.start))
        chunk.intro = indentPiece(chunk.intro);
      if (chunk.edited) {
        if (!isExcluded.has(chunk.start))
          chunk.content = indentPiece(chunk.content);
        else if (chunk.content)
          shouldIndentNextCharacter = chunk.content[chunk.content.length - 1] === `
`;
      } else if (options.exclude) {
        charIndex = chunk.start;
        while (charIndex < end) {
          if (!isExcluded.has(charIndex)) {
            const char = this.original.charCodeAt(charIndex);
            if (char === NEWLINE_CHAR)
              shouldIndentNextCharacter = true;
            else if (char !== CR_CHAR && shouldIndentNextCharacter)
              indentAt(charIndex);
          }
          charIndex += 1;
        }
      } else {
        charIndex = chunk.start;
        while (charIndex < end) {
          if (!shouldIndentNextCharacter) {
            const nextLine = this.original.indexOf(n, charIndex);
            if (nextLine === -1 || nextLine >= end)
              break;
            shouldIndentNextCharacter = true;
            charIndex = nextLine + 1;
            continue;
          }
          const char = this.original.charCodeAt(charIndex);
          if (char === NEWLINE_CHAR || char === CR_CHAR) {
            charIndex += 1;
            continue;
          }
          indentAt(charIndex);
          charIndex += 1;
        }
      }
      if (!isExcluded.has(chunk.end - 1))
        chunk.outro = indentPiece(chunk.outro);
      charIndex = chunk.end;
      chunk = chunk.next;
    }
    this.outro = indentPiece(this.outro);
    return this;
  }
  insert() {
    throw new MagicStringError("insert() is deprecated, use appendLeft() or prependRight()");
  }
  insertLeft(index, content) {
    if (!warned.insertLeft) {
      console.warn("magicString.insertLeft(...) is deprecated. Use magicString.appendLeft(...) instead");
      warned.insertLeft = true;
    }
    return this.appendLeft(index, content);
  }
  insertRight(index, content) {
    if (!warned.insertRight) {
      console.warn("magicString.insertRight(...) is deprecated. Use magicString.prependRight(...) instead");
      warned.insertRight = true;
    }
    return this.prependRight(index, content);
  }
  move(start, end, index, affinity = "right") {
    start = this._resolveIndex(start);
    end = this._resolveIndex(end);
    index = this._resolveIndex(index);
    if (start === end)
      return this;
    if (start < 0 || end > this.original.length || index < 0 || index > this.original.length)
      throw new MagicStringError(`range ${start}–${end} is out of bounds`);
    if (start > end)
      throw new MagicStringError(`end must be greater than start (start: ${start}, end: ${end})`);
    if (index >= start && index <= end)
      throw new MagicStringError("cannot move a selection inside itself");
    this._split(start);
    this._split(end);
    this._split(index);
    const first = this.byStart.get(start);
    const last = this.byEnd.get(end);
    if (!first || !last)
      throw new MagicStringError(`range ${start}–${end} is out of bounds`);
    if (this.hasMovedChunks) {
      let cursor = first;
      while (cursor !== last) {
        cursor = cursor.next;
        if (!cursor || cursor.start < start || cursor.end > end)
          throw new MagicStringError(`cannot move ${start} to ${end} because an earlier move split that range`);
      }
    }
    const oldLeft = first.previous;
    const oldRight = last.next;
    let newLeft;
    let newRight;
    if (affinity === "left") {
      newLeft = this.byEnd.get(index) ?? null;
      if (!newLeft) {
        if (first === this.firstChunk)
          return this;
        newRight = this.firstChunk;
      } else {
        if (newLeft.next === first)
          return this;
        newRight = newLeft.next;
      }
    } else {
      newRight = this.byStart.get(index) ?? null;
      if (!newRight) {
        if (last === this.lastChunk)
          return this;
        newLeft = this.lastChunk;
      } else {
        if (newRight.previous === last)
          return this;
        newLeft = newRight.previous;
      }
    }
    if (oldLeft)
      oldLeft.next = oldRight;
    if (oldRight)
      oldRight.previous = oldLeft;
    if (newLeft)
      newLeft.next = first;
    if (newRight)
      newRight.previous = last;
    if (!oldLeft)
      this.firstChunk = oldRight;
    if (!oldRight) {
      this.lastChunk = oldLeft;
      this.lastChunk.next = null;
    }
    first.previous = newLeft;
    last.next = newRight || null;
    if (!newLeft)
      this.firstChunk = first;
    if (!newRight)
      this.lastChunk = last;
    this.hasMovedChunks = true;
    return this;
  }
  overwrite(start, end, content, options) {
    if (options === true) {
      if (!warned.storeName) {
        console.warn("The final argument to magicString.overwrite(...) should be an options object. See https://github.com/rich-harris/magic-string");
        warned.storeName = true;
      }
      return this.update(start, end, content, {
        storeName: true,
        overwrite: true
      });
    }
    const optionObject = typeof options === "object" && options ? options : {};
    return this.update(start, end, content, {
      ...optionObject,
      overwrite: !optionObject.contentOnly
    });
  }
  update(start, end, content, options) {
    start = this._resolveIndex(start);
    end = this._resolveIndex(end);
    if (typeof content !== "string")
      throw new MagicStringError(`content must be a string, got ${typeof content}`);
    if (start < 0)
      throw new MagicStringError(`start ${start} is out of bounds`);
    if (end > this.original.length)
      throw new MagicStringError(`end ${end} is out of bounds`);
    if (start === end)
      throw new MagicStringError(`cannot overwrite a zero-length range at ${start}, use appendLeft() or prependRight()`);
    if (start > end)
      throw new MagicStringError(`end must be greater than start (start: ${start}, end: ${end})`);
    this._split(start);
    this._split(end);
    if (options === true) {
      if (!warned.storeName) {
        console.warn("The final argument to magicString.overwrite(...) should be an options object. See https://github.com/rich-harris/magic-string");
        warned.storeName = true;
      }
      options = { storeName: true };
    }
    const optionObject = typeof options === "object" && options ? options : {};
    const storeName = optionObject.storeName || false;
    const overwrite = optionObject.overwrite || false;
    if (storeName)
      this.storedNames.add(this.original.slice(start, end));
    const first = this.byStart.get(start);
    for (let chunk = first;chunk.end < end; ) {
      const next = chunk.next;
      if (next === null || next !== this.byStart.get(chunk.end))
        throw new MagicStringError("cannot overwrite across a split point");
      chunk = next;
    }
    for (let chunk = first.next;chunk !== null && chunk.start < end; chunk = chunk.next) {
      chunk.edit("", false, !overwrite);
      chunk.replacesUntil = -1;
      chunk.replacedFrom = start;
    }
    first.edit(content, storeName, !overwrite);
    first.replacesUntil = end;
    first.replacedFrom = -1;
    return this;
  }
  prepend(content) {
    if (typeof content !== "string")
      throw new MagicStringError(`content must be a string, got ${typeof content}`);
    this.intro = content + this.intro;
    return this;
  }
  prependLeft(index, content) {
    index = this._resolveIndex(index);
    if (index < 0 || index > this.original.length)
      throw new MagicStringError(`index ${index} is out of bounds`);
    if (typeof content !== "string")
      throw new MagicStringError(`content must be a string, got ${typeof content}`);
    this._split(index);
    const chunk = this.byEnd.get(index);
    if (chunk)
      chunk.prependLeft(content);
    else
      this.intro = content + this.intro;
    return this;
  }
  prependRight(index, content) {
    index = this._resolveIndex(index);
    if (index < 0 || index > this.original.length)
      throw new MagicStringError(`index ${index} is out of bounds`);
    if (typeof content !== "string")
      throw new MagicStringError(`content must be a string, got ${typeof content}`);
    this._split(index);
    const chunk = this.byStart.get(index);
    if (chunk)
      chunk.prependRight(content);
    else
      this.outro = content + this.outro;
    return this;
  }
  remove(start, end) {
    start = this._resolveIndex(start);
    end = this._resolveIndex(end);
    if (start === end)
      return this;
    if (start < 0 || end > this.original.length)
      throw new MagicStringError(`range ${start}–${end} is out of bounds`);
    if (start > end)
      throw new MagicStringError(`end must be greater than start (start: ${start}, end: ${end})`);
    this._split(start);
    this._split(end);
    let chunk = this.byStart.get(start);
    while (chunk) {
      if (chunk.start > start)
        chunk.intro = "";
      if (chunk.end < end)
        chunk.outro = "";
      chunk.edit("", false, true);
      chunk.replacesUntil = -1;
      chunk.replacedFrom = -1;
      chunk = end > chunk.end ? this.byStart.get(chunk.end) : undefined;
    }
    return this;
  }
  reset(start, end) {
    start = this._resolveIndex(start);
    end = this._resolveIndex(end);
    if (start === end)
      return this;
    if (start < 0 || end > this.original.length)
      throw new MagicStringError(`range ${start}–${end} is out of bounds`);
    if (start > end)
      throw new MagicStringError(`end must be greater than start (start: ${start}, end: ${end})`);
    this._split(start);
    this._split(end);
    let chunk = this.byStart.get(start);
    while (chunk) {
      chunk.reset();
      chunk = end > chunk.end ? this.byStart.get(chunk.end) : undefined;
    }
    return this;
  }
  lastChar() {
    if (this.outro.length)
      return this.outro[this.outro.length - 1];
    let chunk = this.lastChunk;
    while (chunk) {
      if (chunk.outro.length)
        return chunk.outro[chunk.outro.length - 1];
      if (chunk.content.length)
        return chunk.content[chunk.content.length - 1];
      if (chunk.intro.length)
        return chunk.intro[chunk.intro.length - 1];
      chunk = chunk.previous;
    }
    if (this.intro.length)
      return this.intro[this.intro.length - 1];
    return "";
  }
  lastLine() {
    let lineIndex = this.outro.lastIndexOf(n);
    if (lineIndex !== -1)
      return this.outro.slice(lineIndex + 1);
    let lineStr = this.outro;
    let chunk = this.lastChunk;
    while (chunk) {
      if (chunk.outro.length > 0) {
        lineIndex = chunk.outro.lastIndexOf(n);
        if (lineIndex !== -1)
          return chunk.outro.slice(lineIndex + 1) + lineStr;
        lineStr = chunk.outro + lineStr;
      }
      if (chunk.content.length > 0) {
        lineIndex = chunk.content.lastIndexOf(n);
        if (lineIndex !== -1)
          return chunk.content.slice(lineIndex + 1) + lineStr;
        lineStr = chunk.content + lineStr;
      }
      if (chunk.intro.length > 0) {
        lineIndex = chunk.intro.lastIndexOf(n);
        if (lineIndex !== -1)
          return chunk.intro.slice(lineIndex + 1) + lineStr;
        lineStr = chunk.intro + lineStr;
      }
      chunk = chunk.previous;
    }
    lineIndex = this.intro.lastIndexOf(n);
    if (lineIndex !== -1)
      return this.intro.slice(lineIndex + 1) + lineStr;
    return this.intro + lineStr;
  }
  slice(start = 0, end = this.original.length - this.offset) {
    start = this._resolveIndex(start);
    end = this._resolveIndex(end);
    let result = "";
    let chunk;
    if (this.hasMovedChunks) {
      chunk = this.firstChunk;
      while (chunk && (chunk.start > start || chunk.end <= start)) {
        if (chunk.start < end && chunk.end >= end || end === 0 && chunk.start === 0)
          return result;
        chunk = chunk.next;
      }
    } else {
      if (start >= end)
        return result;
      chunk = this._chunkAt(start);
    }
    if (chunk && chunk.edited && chunk.start !== start)
      throw new MagicStringError(`cannot use edited character ${start} as slice start anchor`);
    const startChunk = chunk;
    while (chunk) {
      if (end === 0 && chunk.start === 0)
        break;
      if (chunk.intro && (startChunk !== chunk || chunk.start === start))
        result += chunk.intro;
      const containsEnd = chunk.start < end && chunk.end >= end;
      if (containsEnd && chunk.edited && chunk.end !== end)
        throw new MagicStringError(`cannot use edited character ${end} as slice end anchor`);
      const sliceStart = startChunk === chunk ? start - chunk.start : 0;
      const sliceEnd = containsEnd ? chunk.content.length + end - chunk.end : chunk.content.length;
      result += chunk.content.slice(sliceStart, sliceEnd);
      if (chunk.outro && (!containsEnd || chunk.end === end))
        result += chunk.outro;
      if (containsEnd)
        break;
      chunk = chunk.next;
    }
    return result;
  }
  snip(start, end) {
    const clone = this.clone();
    clone.remove(-clone.offset, start);
    clone.remove(end, clone.original.length - clone.offset);
    return clone;
  }
  _resolveIndex(index) {
    const resolved = index + this.offset;
    if (!Number.isInteger(resolved))
      throw new MagicStringError(`index must be an integer, got ${index}`);
    if (resolved < 0 && this.original.length !== 0)
      return Math.max(0, resolved + this.original.length);
    return resolved;
  }
  _chunkAt(index) {
    let chunk = this.byStart.get(index);
    if (chunk)
      return chunk;
    chunk = this.lastSearchedChunk;
    const searchForward = index >= chunk.end;
    while (chunk && !chunk.contains(index))
      chunk = searchForward ? this.byStart.get(chunk.end) : this.byEnd.get(chunk.start);
    if (chunk)
      this.lastSearchedChunk = chunk;
    return chunk;
  }
  _split(index) {
    if (this.byStart.get(index) || this.byEnd.get(index))
      return;
    let chunk = this.lastSearchedChunk;
    let previousChunk = chunk;
    const searchForward = index > chunk.end;
    while (chunk) {
      if (chunk.contains(index))
        return this._splitChunk(chunk, index);
      chunk = searchForward ? this.byStart.get(chunk.end) : this.byEnd.get(chunk.start);
      if (chunk === previousChunk)
        return;
      previousChunk = chunk;
    }
  }
  _splitChunk(chunk, index) {
    if (chunk.edited && chunk.content.length) {
      const loc = getLocator(this.original)(index);
      throw new MagicStringError(`cannot split a chunk that has already been edited (${loc.line}:${loc.column} – "${chunk.original}")`);
    }
    const newChunk = chunk.split(index);
    this.byEnd.set(index, chunk);
    this.byStart.set(index, newChunk);
    this.byEnd.set(newChunk.end, newChunk);
    if (chunk === this.lastChunk)
      this.lastChunk = newChunk;
    this.lastSearchedChunk = chunk;
    return true;
  }
  toString() {
    let str = this.intro;
    let chunk = this.firstChunk;
    while (chunk) {
      str += chunk.toString();
      chunk = chunk.next;
    }
    return str + this.outro;
  }
  isEmpty() {
    if (this.intro.length && this.intro.trim())
      return false;
    let chunk = this.firstChunk;
    while (chunk) {
      if (chunk.intro.length && chunk.intro.trim() || chunk.content.length && chunk.content.trim() || chunk.outro.length && chunk.outro.trim())
        return false;
      chunk = chunk.next;
    }
    if (this.outro.length && this.outro.trim())
      return false;
    return true;
  }
  length() {
    let chunk = this.firstChunk;
    let length = 0;
    while (chunk) {
      length += chunk.intro.length + chunk.content.length + chunk.outro.length;
      chunk = chunk.next;
    }
    return length;
  }
  trimLines() {
    return this.trim("[\\r\\n]");
  }
  trim(charType) {
    return this.trimStart(charType).trimEnd(charType);
  }
  trimEndAborted(charType) {
    const rx = new RegExp(`${charType || "\\s"}+$`);
    this.outro = this.outro.replace(rx, "");
    if (this.outro.length)
      return true;
    let chunk = this.lastChunk;
    do {
      const end = chunk.end;
      const aborted = chunk.trimEnd(rx);
      if (chunk.end !== end) {
        const next = chunk.next;
        if (this.lastChunk === chunk)
          this.lastChunk = next;
        this.byEnd.set(chunk.end, chunk);
        this.byStart.set(next.start, next);
        this.byEnd.set(next.end, next);
      }
      if (aborted)
        return true;
      chunk = chunk.previous;
    } while (chunk);
    this.intro = this.intro.replace(rx, "");
    return this.intro.length > 0;
  }
  trimEnd(charType) {
    this.trimEndAborted(charType);
    return this;
  }
  trimStartAborted(charType) {
    const rx = new RegExp(`^${charType || "\\s"}+`);
    this.intro = this.intro.replace(rx, "");
    if (this.intro.length)
      return true;
    let chunk = this.firstChunk;
    do {
      const end = chunk.end;
      const aborted = chunk.trimStart(rx);
      if (chunk.end !== end) {
        const next = chunk.next;
        if (chunk === this.lastChunk)
          this.lastChunk = next;
        this.byEnd.set(chunk.end, chunk);
        this.byStart.set(next.start, next);
        this.byEnd.set(next.end, next);
      }
      if (aborted)
        return true;
      chunk = chunk.next;
    } while (chunk);
    this.outro = this.outro.replace(rx, "");
    return this.outro.length > 0;
  }
  trimStart(charType) {
    this.trimStartAborted(charType);
    return this;
  }
  hasChanged() {
    if (this.intro || this.outro)
      return true;
    let outputIndex = 0;
    let chunk = this.firstChunk;
    while (chunk) {
      if (chunk.intro || chunk.outro)
        return true;
      if (!chunk.edited && chunk.start === outputIndex)
        outputIndex = chunk.end;
      else {
        if (!this.original.startsWith(chunk.content, outputIndex))
          return true;
        outputIndex += chunk.content.length;
      }
      chunk = chunk.next;
    }
    return outputIndex !== this.original.length;
  }
  _hasRemovedContent(start, end) {
    const atBoundary = this.byStart.get(start);
    if (atBoundary && start === end) {
      const previous = this.byEnd.get(start);
      return !!previous && previous.content === "" && atBoundary.content === "";
    }
    let chunk = this._chunkAt(start);
    while (chunk && chunk.start < end) {
      if (chunk.content === "" && (chunk.replacedFrom < start || this.byStart.get(chunk.replacedFrom).replacesUntil < chunk.end))
        return true;
      chunk = this.byStart.get(chunk.end);
    }
    return false;
  }
  _replaceRegexp(searchValue, replacement) {
    function getReplacement(match, index, str) {
      if (typeof replacement === "string")
        return expandReplacement(replacement, match[0], index, str, match.slice(1), match.groups);
      else
        return match.groups === undefined ? replacement(match[0], ...match.slice(1), index, str) : replacement(match[0], ...match.slice(1), index, str, match.groups);
    }
    const replaceMatch = (match) => {
      if (match.index == null)
        return false;
      if (this._hasRemovedContent(match.index, match.index + match[0].length))
        return false;
      const replacement = getReplacement(match, match.index, this.original);
      if (replacement === match[0])
        return true;
      if (match[0].length === 0)
        this.appendRight(match.index - this.offset, replacement);
      else
        this.overwrite(match.index - this.offset, match.index + match[0].length - this.offset, replacement);
      return true;
    };
    if (searchValue.global) {
      searchValue.lastIndex = 0;
      for (const match of this.original.matchAll(searchValue))
        replaceMatch(match);
    } else {
      const match = this.original.match(searchValue);
      if (match && !replaceMatch(match)) {
        const global = new RegExp(searchValue.source, `${searchValue.flags}g`);
        for (const next of this.original.matchAll(global))
          if (replaceMatch(next))
            break;
      }
    }
    return this;
  }
  _replaceString(string, replacement) {
    const { original } = this;
    let index = original.indexOf(string);
    while (index !== -1) {
      if (this._hasRemovedContent(index, index + string.length)) {
        index = original.indexOf(string, index + string.length);
        continue;
      }
      if (typeof replacement === "function")
        replacement = replacement(string, index, original);
      else
        replacement = expandReplacement(replacement, string, index, original, [], undefined);
      if (string !== replacement) {
        if (string.length === 0)
          this.appendRight(index - this.offset, replacement);
        else
          this.overwrite(index - this.offset, index + string.length - this.offset, replacement);
      }
      break;
    }
    return this;
  }
  replace(searchValue, replacement) {
    if (typeof searchValue === "string")
      return this._replaceString(searchValue, replacement);
    return this._replaceRegexp(searchValue, replacement);
  }
  _replaceAllString(string, replacement) {
    const { original } = this;
    const stringLength = string.length;
    if (stringLength === 0) {
      for (let index = 0;index <= original.length; index += 1) {
        if (this._hasRemovedContent(index, index))
          continue;
        const _replacement = typeof replacement === "function" ? replacement("", index, original) : expandReplacement(replacement, "", index, original, [], undefined);
        if (_replacement !== "")
          this.appendRight(index - this.offset, _replacement);
      }
      return this;
    }
    for (let index = original.indexOf(string);index !== -1; index = original.indexOf(string, index + stringLength)) {
      if (this._hasRemovedContent(index, index + stringLength))
        continue;
      const previous = original.slice(index, index + stringLength);
      const _replacement = typeof replacement === "function" ? replacement(previous, index, original) : expandReplacement(replacement, previous, index, original, [], undefined);
      if (previous !== _replacement)
        this.overwrite(index - this.offset, index + stringLength - this.offset, _replacement);
    }
    return this;
  }
  replaceAll(searchValue, replacement) {
    if (typeof searchValue === "string")
      return this._replaceAllString(searchValue, replacement);
    if (!searchValue.global)
      throw new MagicStringError("replaceAll() requires a global RegExp");
    return this._replaceRegexp(searchValue, replacement);
  }
};

// emitter.mjs
function byteIndices(source) {
  const map = new Map([[0, 0]]);
  let bytes = 0, units = 0;
  for (const c of source) {
    bytes += Buffer.byteLength(c);
    units += c.length;
    map.set(bytes, units);
  }
  return map;
}
function instrument(source, candidates, file, runId) {
  const indices = byteIndices(source);
  const root = { start: 0, end: source.length, children: [], candidates: [] };
  const groups = new Map;
  for (const candidate of candidates.filter((c) => !c.ignored)) {
    const start = indices.get(candidate.byteOffset), end = indices.get(candidate.end);
    if (start === undefined || end === undefined || start > end)
      throw new Error(`Invalid UTF-8 range in ${file}`);
    const groupKey = `${start}:${end}`;
    let group = groups.get(groupKey);
    if (!group) {
      group = { start, end, children: [], candidates: [] };
      groups.set(groupKey, group);
    }
    group.candidates.push(candidate);
  }
  const stack = [root];
  for (const group of [...groups.values()].sort((a, b) => a.start - b.start || b.end - a.end)) {
    while (stack.length > 1 && group.start >= stack.at(-1).end)
      stack.pop();
    const parent = stack.at(-1);
    if (group.end > parent.end)
      throw new Error(`Crossing mutation placements in ${file}`);
    parent.children.push(group);
    stack.push(group);
  }
  const runtime = `globalThis[Symbol.for(${JSON.stringify(`errspec.mutation.${runId}`)})]`;
  function slice(group, start, end) {
    let original = source.slice(start, end);
    for (const child of [...group.children].reverse()) {
      if (child.end <= start || child.start >= end)
        continue;
      if (child.start < start || child.end > end)
        throw new Error(`Weak operands cross a mutation placement in ${file}`);
      original = original.slice(0, child.start - start) + render(child) + original.slice(child.end - start);
    }
    return original;
  }
  function render(group) {
    let original = slice(group, group.start, group.end);
    const weak = group.candidates.filter((c) => c.weak);
    if (weak.length) {
      const layout = weak[0].weak;
      const leftEnd = indices.get(layout.leftEnd), rightStart = indices.get(layout.rightStart);
      if (leftEnd === undefined || rightStart === undefined || leftEnd < group.start || rightStart > group.end || leftEnd > rightStart || weak.some((c) => c.weak.leftEnd !== layout.leftEnd || c.weak.rightStart !== layout.rightStart || c.weak.operator !== layout.operator))
        throw new Error(`Invalid weak mutation range in ${file}`);
      const changes = JSON.stringify(weak.map((c) => [c.key, c.weak.replacementOperator]));
      const left = slice(group, group.start, leftEnd), right = slice(group, rightStart, group.end);
      original = `(${runtime}?.binary?${runtime}.binary(${JSON.stringify(layout.operator)},${changes},(${left}),(${right})):(${original}))`;
    }
    for (const c of [...group.candidates].reverse()) {
      if (c.weak)
        continue;
      const check = `${runtime}?.(${JSON.stringify(c.key)})`;
      if (c.placement === "emptyCondition")
        original = `(${check}?false:true)`;
      else if (c.placement === "expression")
        original = `(${check}?(${c.replacement}):(${original}))`;
      else if (c.placement === "body") {
        const prefixLength = indices.get(c.guardOffset) - group.start;
        if (!Number.isInteger(prefixLength) || prefixLength < 1)
          throw new Error("Invalid body guard position");
        const directive = original.slice(1, prefixLength);
        const branch = `${c.replacement.slice(1, -1).trim()}
return;`;
        original = `{${directive}
if(${check}){${branch}}${original.slice(prefixLength)}`;
      } else {
        original = `{if(${check}){${c.replacement}}else{${original}}}`;
      }
    }
    return original;
  }
  const magic = new MagicString(source);
  for (const child of root.children) {
    if (child.start === child.end)
      magic.appendLeft(child.start, render(child));
    else
      magic.overwrite(child.start, child.end, render(child));
  }
  return { code: magic.toString(), map: magic.generateMap({ source: file, includeContent: true, hires: true }) };
}

// typecheck.mjs
import { createRequire } from "node:module";
import { readFileSync, existsSync, realpathSync } from "node:fs";
import { dirname, join, resolve, relative } from "node:path";
import { pathToFileURL } from "node:url";
function packageDirectory(root, explicit) {
  if (explicit)
    return realpathSync(explicit);
  for (let current = root;; current = dirname(current)) {
    const candidate = join(current, "node_modules/typescript");
    if (existsSync(join(candidate, "package.json")))
      return realpathSync(candidate);
    if (dirname(current) === current)
      break;
  }
  throw new Error("Cannot find TypeScript; use --typecheck-package");
}
async function open(root, project, packageDir) {
  const metadata = JSON.parse(readFileSync(join(packageDir, "package.json"), "utf8"));
  const load = createRequire(join(packageDir, "package.json"));
  const overrides = new Map, versions = new Map;
  let compiler, api, snapshot, service, parsed;
  const close = () => {
    try {
      service?.dispose();
      snapshot?.dispose();
    } finally {
      api?.close();
    }
  };
  try {
    const major = Number(metadata.version.split(".")[0]);
    if (major <= 6 && major > 0) {
      compiler = load(packageDir);
      parsed = compiler.getParsedCommandLineOfConfigFile(project, {}, {
        ...compiler.sys,
        onUnRecoverableConfigFileDiagnostic: (diagnostic) => {
          throw new Error(compiler.flattenDiagnosticMessageText(diagnostic.messageText, `
`));
        }
      });
      if (!parsed || parsed.errors.length)
        throw new Error("Cannot parse typecheck project: " + JSON.stringify(parsed?.errors.map((d) => d.messageText)));
      const host = {
        getCompilationSettings: () => parsed.options,
        getScriptFileNames: () => parsed.fileNames,
        getScriptVersion: (file) => String(versions.get(file) || 0),
        getScriptSnapshot: (file) => {
          const text = overrides.get(file) ?? compiler.sys.readFile(file);
          return text === undefined ? undefined : compiler.ScriptSnapshot.fromString(text);
        },
        getCurrentDirectory: () => dirname(project),
        getDefaultLibFileName: (options) => compiler.getDefaultLibFilePath(options),
        fileExists: (file) => overrides.has(file) || compiler.sys.fileExists(file),
        readFile: (file) => overrides.get(file) ?? compiler.sys.readFile(file),
        readDirectory: compiler.sys.readDirectory,
        directoryExists: compiler.sys.directoryExists,
        getDirectories: compiler.sys.getDirectories
      };
      service = compiler.createLanguageService(host, compiler.createDocumentRegistry());
    } else if (major === 7) {
      compiler = await import(pathToFileURL(load.resolve(`${metadata.name}/unstable/sync`)).href);
      api = new compiler.API({
        cwd: root,
        fs: { readFile: (file) => overrides.get(file) }
      });
      snapshot = api.updateSnapshot({ openProjects: [project] });
    } else
      throw new Error(`Unsupported mutant typechecker: TypeScript ${metadata.version}`);
    const context = () => {
      if (api) {
        const selected = snapshot.getProject(project);
        if (!selected)
          throw new Error("TypeScript API did not open the requested project");
        return {
          program: selected.program,
          checker: selected.checker,
          options: selected.compilerOptions
        };
      }
      const program = service.getProgram();
      if (!program)
        throw new Error("TypeScript did not create a language-service program");
      return {
        program,
        checker: program.getTypeChecker(),
        options: parsed.options
      };
    };
    if (context().options.noCheck)
      throw new Error("noCheck must be false for mutant typechecking");
    const diagnose = () => {
      const { program } = context();
      const diagnostics = api ? [
        ...program.getConfigFileParsingDiagnostics(),
        ...program.getProgramDiagnostics(),
        ...program.getGlobalDiagnostics(),
        ...program.getSyntacticDiagnostics(),
        ...program.getSemanticDiagnostics()
      ] : compiler.getPreEmitDiagnostics(program);
      return diagnostics.filter((d) => d.category === 1).map((d) => {
        const message = api ? nativeMessage(d) : compiler.flattenDiagnosticMessageText(d.messageText, `
`);
        const file = d.file?.fileName ?? d.fileName;
        return `${file ? relative(root, file) : "<project>"}: TS${d.code} ${message}`;
      });
    };
    const change = (file, text) => {
      if (text === undefined)
        overrides.delete(file);
      else
        overrides.set(file, text);
      versions.set(file, (versions.get(file) || 0) + 1);
      if (api) {
        const next = api.updateSnapshot({ fileChanges: { changed: [file] } });
        snapshot.dispose();
        snapshot = next;
      }
    };
    return {
      version: metadata.version,
      compiler,
      context,
      diagnose,
      change,
      close
    };
  } catch (error) {
    close();
    throw error;
  }
}
function nativeMessage(diagnostic) {
  if (!Number.isInteger(diagnostic.code) || typeof diagnostic.text !== "string")
    throw new Error("Unsupported TypeScript diagnostic format");
  return [
    diagnostic.text,
    ...(diagnostic.messageChain || []).map(nativeMessage)
  ].join(`
`);
}
function newDiagnostic(before, after) {
  const counts = new Map;
  for (const message of before)
    counts.set(message, (counts.get(message) || 0) + 1);
  for (const message of after) {
    const count = counts.get(message) || 0;
    if (!count)
      return message;
    counts.set(message, count - 1);
  }
}
function applyCandidate(source, candidate) {
  const indices = byteIndices(source), start = indices.get(candidate.byteOffset), end = indices.get(candidate.end);
  if (start === undefined || end === undefined || start > end || source.slice(start, end) !== candidate.original)
    throw new Error(`Invalid original text or UTF-8 range for ${candidate.key}`);
  return source.slice(0, start) + candidate.replacement + source.slice(end);
}
function equivalents(checker, file, source, candidates) {
  const { program, checker: types, options } = checker.context(), T = checker.compiler.TypeFlags;
  if (!options.noUncheckedIndexedAccess || types.isTypeAssignableTo(types.getUndefinedType(), types.getStringType()))
    return new Map;
  const ast = program.getSourceFile(file);
  if (!ast)
    throw new Error(`Typecheck source is outside the project: ${file}`);
  const chains = new Map;
  const visit = (node) => {
    if (node.questionDotToken && node.expression)
      chains.set(node.questionDotToken.getStart(ast), node.expression);
    node.forEachChild((child) => {
      visit(child);
    });
  };
  visit(ast);
  const primitive = [
    "String",
    "Number",
    "BigInt",
    "Boolean",
    "ESSymbol",
    "StringLiteral",
    "NumberLiteral",
    "BigIntLiteral",
    "BooleanLiteral",
    "UniqueESSymbol",
    "EnumLiteral",
    "Enum",
    "NonPrimitive",
    "Object",
    "TemplateLiteral",
    "StringMapping"
  ].reduce((mask, name) => mask | T[name], 0);
  const excludesNull = (type) => {
    if (type.flags & (T.Union | T.Intersection)) {
      const members = type.getTypes ? type.getTypes() : type.types;
      return type.flags & T.Union ? members.every(excludesNull) : members.some(excludesNull);
    }
    return !!(type.flags & primitive);
  };
  const result = new Map, indices = byteIndices(source);
  for (const candidate of candidates.filter((c) => c.mutator === "OptionalChaining")) {
    let prefix = 0;
    while (prefix < candidate.original.length && candidate.original[prefix] === candidate.replacement[prefix])
      prefix++;
    if (candidate.original.slice(prefix, prefix + 2) !== "?.")
      continue;
    const target = chains.get(indices.get(candidate.byteOffset) + prefix - (source.startsWith("\uFEFF") ? 1 : 0));
    if (target && excludesNull(types.getTypeAtLocation(target)))
      result.set(candidate.key, `equivalent: ${target.getText(ast)} excludes null and undefined`);
  }
  return result;
}
async function typecheckMutants(options, inventory, progress = () => {}) {
  const root = options.root;
  const project = realpathSync(options.typecheckProject || join(root, "tsconfig.json"));
  const packageDir = packageDirectory(root, options.typecheckPackage);
  const checker = await open(root, project, packageDir);
  try {
    const baseline = checker.diagnose(), outcomes = [];
    for (const [file, entry] of inventory) {
      if (!/\.[cm]?tsx?$/.test(file))
        continue;
      const candidates = entry.candidates.filter((c) => !c.ignored);
      if (!candidates.length)
        continue;
      if (!checker.context().program.getSourceFile(file))
        throw new Error(`Typecheck source is outside the project: ${file}`);
      const equivalent = equivalents(checker, file, entry.source, candidates);
      for (const candidate of candidates) {
        let reason = equivalent.get(candidate.key), status = reason ? "Ignored" : "checked";
        if (!reason) {
          try {
            checker.change(file, applyCandidate(entry.source, candidate));
            reason = newDiagnostic(baseline, checker.diagnose());
          } finally {
            checker.change(file, undefined);
          }
          if (reason)
            status = "CompileError";
        }
        outcomes.push({ key: candidate.key, status, reason: reason ?? null });
        progress(candidate.key);
      }
    }
    return {
      info: {
        version: checker.version,
        package: packageDir,
        project,
        checked: outcomes.length,
        compileErrors: outcomes.filter((r) => r.status === "CompileError").length,
        equivalent: outcomes.filter((r) => r.status === "Ignored").length
      },
      outcomes
    };
  } finally {
    checker.close();
  }
}

// cache-inputs.mjs
var builtinCompilationPlugins = new Set(`
errspec-mutation alias vite:optimized-deps vite:pre-alias vitest:resolve-builtin
vitest:css-disable vitest:resolve-core vitest:meta-env-replacer
vite:modulepreload-polyfill vite:bundled-dev-client-imports vite:resolve-dev
vite:html-inline-proxy vite:css builtin:oxc-runtime vite:oxc builtin:vite-json
vite:wasm-helper vite:worker vite:asset vite:define vite:css-post vite:build-html
vite:worker-import-meta-url vite:asset-import-meta-url vite:dynamic-import-vars
vite:import-glob vitest:css-empty-post vitest:mocks vitest:automock
vitest:coverage-transform vitest:normalize-url vite:client-inject
vite:css-analysis vite:import-analysis builtin:vite-resolve
`.trim().split(/\s+/));
function knownCompilation(plugins = []) {
  const seen = new Set;
  const order = [];
  for (const plugin of plugins) {
    if (!plugin.transform && !plugin.load && !plugin.resolveId)
      continue;
    if (!builtinCompilationPlugins.has(plugin.name) || seen.has(plugin.name))
      throw new Error("Unacquired compilation plugin");
    seen.add(plugin.name);
    order.push(plugin.name);
  }
  return order;
}
function compilerInputs(server, test, relocatedPath) {
  if (Object.keys(test._scriptDefines || {}).length)
    throw new Error("Runtime define scripts cannot be fingerprinted");
  const css = test.css;
  const processCss = css === true || css && typeof css === "object" && css.include?.length > 0;
  const assets = (config) => {
    if (typeof config.assetsInclude !== "function")
      return config.assetsInclude;
    const implementation = Function.prototype.toString.call(config.assetsInclude).replace(/\s+/g, "");
    if (Array.isArray(config.rawAssetsInclude) && /^assetsInclude\(file\)\{returnDEFAULT_ASSETS_RE(?:\$\d+)?\.test\(file\)\|\|assetsFilter\(file\);?\}$/.test(implementation))
      return { builtin: "vite-assets", patterns: config.rawAssetsInclude };
    throw new Error("Asset matching cannot be fingerprinted");
  };
  const resolve = (config) => ({
    ...config,
    ...Array.isArray(config?.alias) ? { alias: config.alias.map((alias) => ({
      ...alias,
      ...typeof alias.replacement === "string" ? { replacement: resolutionPath(alias.replacement) } : {}
    })) } : {}
  });
  function resolutionPath(path) {
    if (path.startsWith("/@fs/")) {
      const location = relocatedPath(path.slice("/@fs".length));
      if (location && typeof location === "object")
        return { viteFilesystemPath: location };
      return path;
    }
    return relocatedPath(path);
  }
  const compilation = (config) => {
    const compilationPlugins = knownCompilation(config.plugins);
    return {
      compilationPlugins,
      define: config.define,
      mode: config.mode,
      isProduction: config.isProduction,
      base: config.base,
      appType: config.appType,
      assetsInclude: assets(config),
      env: config.env,
      envPrefix: config.envPrefix,
      publicDir: typeof config.publicDir === "string" ? relocatedPath(config.publicDir) : config.publicDir,
      envDir: typeof config.envDir === "string" ? relocatedPath(config.envDir) : config.envDir,
      resolve: resolve(config.resolve),
      esbuild: config.esbuild,
      oxc: config.oxc,
      json: config.json,
      ssr: config.ssr,
      experimental: config.experimental,
      optimizeDeps: config.optimizeDeps,
      testCss: css,
      ...processCss ? { css: config.css } : {}
    };
  };
  return {
    runtimeDefines: test.defines,
    root: compilation(server.config),
    environments: Object.fromEntries(Object.entries(server.environments || {}).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0).map(([name, environment]) => [name, compilation(environment.config)]))
  };
}

// plugins.mjs
import { readFileSync as readFileSync2, existsSync as existsSync2, realpathSync as realpathSync2 } from "node:fs";
import { dirname as dirname2, resolve as resolve2, join as join2, isAbsolute } from "node:path";
import { pathToFileURL as pathToFileURL2, fileURLToPath } from "node:url";
import { createRequire as createRequire2, registerHooks } from "node:module";
var builtin = new Set(["ArithmeticOperator", "ArrayDeclaration", "ArrowFunction", "AssignmentOperator", "BlockStatement", "BooleanLiteral", "CallExpression", "ConditionalExpression", "EqualityOperator", "FnValue", "LogicalOperator", "MethodExpression", "ObjectLiteral", "OptionalChaining", "Regex", "StringLiteral", "UnaryOperator", "UpdateOperator"]);
var fail = (message) => Object.assign(new Error(message), { code: "EC_MUTATION_CONFIG" });
var validName = (name) => typeof name === "string" && /^[A-Za-z0-9_.-]+$/.test(name);
async function loadPlugins(specs, root, remember, excluded = []) {
  const mutators = [], ignorers = [], aridCallees = [], names = new Set, ignoreNames = new Set;
  let cacheable = true;
  const tracked = new Set;
  function acquire(file, module = false, content) {
    file = realpathSync2(file);
    remember(file, content);
    if (module)
      tracked.add(file);
    for (let dir = dirname2(file);; dir = dirname2(dir)) {
      if (existsSync2(join2(dir, "package.json")))
        remember(join2(dir, "package.json"));
      if (dirname2(dir) === dir)
        break;
    }
  }
  function add(value, settings, origin, inheritedCache = false, inputRoot = root) {
    if (Array.isArray(value)) {
      for (const part of value)
        add(part, settings, origin, inheritedCache, inputRoot);
      return;
    }
    if (!value || typeof value !== "object" || !validName(value.name))
      throw fail(`Plugin ${origin} must export a named mutator, ignorer or plugin`);
    const deterministic = inheritedCache || value.cache === "deterministic";
    cacheable &&= deterministic;
    if (value.inputs !== undefined && (!Array.isArray(value.inputs) || value.inputs.some((p) => typeof p !== "string" || !p)))
      throw fail(`Plugin ${value.name} inputs must be file paths`);
    for (const path of value.inputs || [])
      acquire(resolve2(inputRoot, path));
    const context = { settings, root, readInput(path) {
      const file = resolve2(inputRoot, path), content = readFileSync2(file);
      acquire(file, false, content);
      return content.toString("utf8");
    } };
    if (typeof value.visit === "function") {
      if (builtin.has(value.name) || names.has(value.name))
        throw fail(`Custom mutator ${value.name} duplicates a built-in or custom mutator`);
      names.add(value.name);
      mutators.push({ definition: value, context });
      return;
    }
    if (typeof value.shouldIgnore === "function") {
      if (ignoreNames.has(value.name))
        throw fail(`Duplicate plugin ignorer ${value.name}`);
      ignoreNames.add(value.name);
      ignorers.push({ definition: value, context });
      return;
    }
    if (!value.mutators && !value.ignorers && !value.aridCallees)
      throw fail(`Plugin ${value.name} has no mutators, ignorers or aridCallees`);
    for (const key of ["mutators", "ignorers", "aridCallees"])
      if (value[key] !== undefined && !Array.isArray(value[key]))
        throw fail(`Plugin ${value.name} ${key} must be an array`);
    for (const part of [...value.mutators || [], ...value.ignorers || []])
      add(part, settings, origin, deterministic, inputRoot);
    aridCallees.push(...value.aridCallees || []);
  }
  const observe = () => registerHooks({
    resolve(specifier, context, next) {
      const result = next(specifier, context);
      if (result.url.startsWith("file:"))
        acquire(fileURLToPath(result.url), true);
      return result;
    },
    load(url, context, next) {
      if (url.startsWith("file:"))
        acquire(fileURLToPath(url), true);
      return next(url, context);
    }
  });
  const hooks = observe();
  try {
    const require2 = createRequire2(join2(root, "__errspec_plugin__.mjs"));
    for (const spec of specs || []) {
      const module = typeof spec === "string" ? spec : spec.module;
      const settings = typeof spec === "string" ? null : spec.settings ?? null;
      const file = realpathSync2(module.startsWith(".") || isAbsolute(module) ? resolve2(root, module) : require2.resolve(module));
      acquire(file, true);
      const loaded = await import(pathToFileURL2(file).href);
      add(loaded.default ?? loaded, settings, module, false, dirname2(file));
    }
  } catch (error) {
    throw fail(`Cannot load mutation plugin: ${error.message}`);
  } finally {
    hooks.deregister();
  }
  for (const name of excluded)
    if (!builtin.has(name) && !names.has(name))
      throw fail(`Unknown mutator: ${name}`);
  return {
    names: [...names],
    aridCallees,
    cacheable,
    files: tracked,
    async edits(source, file, metadata) {
      const hooks = observe();
      try {
        const bytes = Buffer.from(source);
        const nodes = Object.freeze(metadata.nodes.map((node, id) => Object.freeze({
          ...node,
          id,
          children: Object.freeze([...node.children]),
          text: bytes.subarray(node.start, node.end).toString("utf8")
        })));
        const edits = [], ignores = [];
        const context = (base, node) => ({
          ...base,
          file,
          source,
          parent: node.parent === null ? null : nodes[node.parent],
          nodes,
          text(range) {
            return bytes.subarray(range.start, range.end).toString("utf8");
          }
        });
        for (const node of nodes) {
          for (const { definition, context: base } of mutators) {
            if (excluded.includes(definition.name))
              continue;
            let result;
            try {
              result = await definition.visit(node, context(base, node));
            } catch (error) {
              throw fail(`Plugin mutator ${definition.name} failed in ${file} at ${node.start}: ${error.message}`);
            }
            if (result == null)
              continue;
            if (!Array.isArray(result))
              throw fail(`Plugin mutator ${definition.name} must return an array of edits`);
            for (const edit of result) {
              const start = edit?.range?.start ?? node.start, end = edit?.range?.end ?? node.end;
              if (typeof edit?.replacement !== "string" || !Number.isSafeInteger(start) || !Number.isSafeInteger(end))
                throw fail(`Plugin mutator ${definition.name} returned an invalid edit`);
              edits.push({ mutator: definition.name, anchorStart: node.start, anchorEnd: node.end, anchorKind: node.kind, start, end, replacement: edit.replacement });
            }
          }
          for (const { definition, context: base } of ignorers) {
            let reason;
            try {
              reason = await definition.shouldIgnore(node, context(base, node));
            } catch (error) {
              throw fail(`Plugin ignorer ${definition.name} failed in ${file} at ${node.start}: ${error.message}`);
            }
            if (reason == null || reason === false)
              continue;
            if (typeof reason !== "string" || !reason.trim())
              throw fail(`Plugin ignorer ${definition.name} must return a nonempty reason`);
            ignores.push({ start: node.start, end: node.end, reason: `${definition.name}: ${reason}` });
          }
        }
        return { pluginEdits: edits, pluginIgnores: ignores };
      } finally {
        hooks.deregister();
      }
    }
  };
}

// driver.mjs
var protocolWrite = process.stdout.write.bind(process.stdout);
process.stdout.write = (chunk, encoding, cb) => process.stderr.write(chunk, encoding, cb);
console.log = (...args) => process.stderr.write(format(...args) + `
`);
console.info = console.log;
var PROTOCOL = 1;
var options;
var root;
var runId;
var api;
var packageDir;
var toolTemp;
var inventory = new Map;
var fingerprints = new Map;
var plain;
var baseline;
var testSpecs;
var projectName;
var configFiles = new Set;
var current;
var infrastructureFailure;
var warmContext;
var reuseMode = "fresh_lifecycle_unknown";
var configurationHash = null;
var pendingGenerated = new Map;
var mutationPlugins;
var normalize = (s) => s.replaceAll("\\", "/");
var rel = (file) => normalize(relative2(root, file));
var hash = (data) => createHash("sha256").update(data).digest("hex");
var relocatedPath = (path) => {
  if (!options.cacheRelocation || !isAbsolute2(path))
    return path;
  const name = rel(path);
  return name !== ".." && !name.startsWith("../") && !isAbsolute2(name) ? { projectRootRelativePath: name } : path;
};
function failure(code, message) {
  const e = new Error(message);
  e.code = code;
  return e;
}
function remember(file, content) {
  if (!existsSync3(file) || !statSync(file).isFile())
    return;
  file = realpathSync3(file);
  const next = hash(readFileSync3(file));
  if (content !== undefined && hash(content) !== next)
    throw failure("EC_MUTATION_INPUT", `Input changed while being acquired: ${file}`);
  if (fingerprints.has(file) && fingerprints.get(file) !== next)
    throw failure("EC_MUTATION_INPUT", `Input changed during mutation run: ${file}`);
  fingerprints.set(file, next);
}
function verifyInputs() {
  for (const [file, digest] of fingerprints) {
    if (!existsSync3(file) || hash(readFileSync3(file)) !== digest)
      throw failure("EC_MUTATION_INPUT", `Input changed during mutation run: ${file}`);
  }
}
function native(source, file, validateOnly = false, pluginInputs = {}, facts = false) {
  const result = spawnSync(options.binary, ["__mutation-source"], {
    input: JSON.stringify({ source, file, validateOnly, excludeMutators: options.excludeMutator, weakMutation: options.weakMutation, arid: options.arid, aridCallees: options.aridCallees, facts: !!(facts || options.since || options.cache && options.cacheScope || options.plugins?.length), customMutators: mutationPlugins?.names, pluginAridCallees: mutationPlugins?.aridCallees, ...pluginInputs, engineFingerprint: options.engineFingerprint }),
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    timeout: 60000
  });
  if (result.status !== 0)
    throw failure(result.stderr?.startsWith("EC_MUTATION_INPUT:") ? "EC_MUTATION_INPUT" : "EC_MUTATION_INSTRUMENTATION", result.stderr || "Native parser failed");
  try {
    return JSON.parse(result.stdout);
  } catch {
    throw failure("EC_MUTATION_PROTOCOL", "Invalid native generator response");
  }
}
async function generateSource(source, file, base) {
  if (!options.plugins?.length)
    return base ?? native(source, file);
  const acquired = base ?? native(source, file);
  const edits = await mutationPlugins.edits(source, file, acquired.metadata);
  return native(source, file, false, edits);
}
function packagePath(explicit) {
  if (explicit) {
    if (!existsSync3(join3(explicit, "package.json")))
      throw failure("EC_MUTATION_CONFIG", `Vitest package not found: ${explicit}`);
    return realpathSync3(explicit);
  }
  for (let dir = root;; dir = dirname3(dir)) {
    const candidate = join3(dir, "node_modules/vitest");
    if (existsSync3(join3(candidate, "package.json")))
      return realpathSync3(candidate);
    if (dirname3(dir) === dir)
      break;
  }
  throw failure("EC_MUTATION_CONFIG", "Cannot find Vitest; use --vitest-package");
}
function candidateFiles(excluded) {
  const patterns = options.include.length ? options.include : ["src/**/*.{ts,tsx,mts,cts,js,jsx,mjs,cjs}"];
  const files = new Set;
  for (const pattern of patterns)
    for (const entry of globSync(pattern, { cwd: root, exclude: ["**/node_modules/**", ...options.exclude] })) {
      const absolute = realpathSync3(resolve3(root, entry));
      const name = rel(absolute);
      if (absolute === toolTemp || absolute.startsWith(toolTemp + "/") || name.startsWith("../") || isAbsolute2(name) || name.split("/").includes("node_modules") || !/\.(?:[cm]?[jt]s|[jt]sx)$/.test(name) || /\.d\.(?:[cm]?ts)$/.test(name) || excluded.has(absolute) || !statSync(absolute).isFile())
        continue;
      files.add(absolute);
    }
  return [...files].sort((a, b) => rel(a) < rel(b) ? -1 : rel(a) > rel(b) ? 1 : 0);
}
function specIdentities(specs) {
  return specs.map((s) => ({ file: s.moduleId, project: s.project.config.name || "" })).sort((a, b) => a.file < b.file ? -1 : a.file > b.file ? 1 : a.project < b.project ? -1 : a.project > b.project ? 1 : 0);
}
function plugin(phase) {
  return { name: "errspec-mutation", enforce: "pre", configResolved(config) {
    if (config.configFile)
      configFiles.add(realpathSync3(config.configFile));
    for (const file of config.configFileDependencies || [])
      configFiles.add(realpathSync3(file));
  }, transform(code, id) {
    let file = id.split("?")[0];
    if (existsSync3(file))
      file = realpathSync3(file);
    const entry = inventory.get(file);
    if (!entry || phase === "plain")
      return;
    if (code !== entry.source) {
      infrastructureFailure = failure("EC_MUTATION_INSTRUMENTATION", `Source was transformed before mutation instrumentation: ${rel(file)}`);
      throw infrastructureFailure;
    }
    return entry.emitted;
  } };
}
var childDependencies = new Map;
function childProcessClosure(entries) {
  const pending = entries.map((file) => resolve3(root, file)), seen = new Set;
  while (pending.length) {
    const file = pending.pop();
    if (seen.has(file))
      continue;
    seen.add(file);
    remember(file);
    let dependencies = childDependencies.get(file);
    if (!dependencies) {
      dependencies = [];
      const source = inventory.get(file)?.source ?? readFileSync3(file, "utf8");
      for (const specifier of native(source, rel(file), true, {}, true).metadata?.dependencies || []) {
        if (!specifier.startsWith("."))
          continue;
        const target = resolve3(dirname3(file), specifier);
        const candidates = [target, ...[".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"].map((ext) => target + ext), ...["index.ts", "index.js", "index.mjs", "index.cjs"].map((name) => join3(target, name))];
        const found = candidates.find((path) => existsSync3(path) && statSync(path).isFile());
        if (found && /[.][cm]?[jt]sx?$/.test(found) && !rel(found).startsWith("../") && !rel(found).split("/").includes("node_modules"))
          dependencies.push(found);
      }
      childDependencies.set(file, dependencies);
    }
    pending.push(...dependencies);
  }
  return [...seen].map(rel).sort();
}
function childProcessEntries(fact) {
  const tokens = fact.method === "execSync" || fact.argv[1] === "-c" ? (fact.argv.at(-1).match(/(?:[^\s"'\\]|\\.|"[^"]*"|'[^']*')+/g) || []).map((v) => v.replace(/^(["'])([\s\S]*)\1$/, "$2").replace(/\\([\s"'])/g, "$1")) : fact.argv.slice(1);
  const entries = new Set;
  for (const arg of tokens) {
    if (!arg || arg.startsWith("-") || !/[.][cm]?[jt]sx?$/.test(arg))
      continue;
    const path = resolve3(fact.cwd, arg), relative = rel(path);
    if (relative.startsWith("../") || relative.split("/").includes("node_modules") || !existsSync3(path))
      continue;
    if (!statSync(path).isFile())
      continue;
    entries.add(relative);
  }
  return childProcessClosure([...entries]);
}
function recordModule(module, execution, request) {
  const counts = new Map;
  const moduleFile = rel(module.moduleId);
  const duration = module.diagnostic();
  execution.moduleDurations[moduleFile] = duration.duration + duration.collectDuration + duration.setupDuration + duration.environmentSetupDuration + duration.prepareDuration;
  for (const test of module.children.allTests()) {
    const occurrence = counts.get(test.fullName) || 0;
    counts.set(test.fullName, occurrence + 1);
    const id = JSON.stringify([projectName, moduleFile, test.fullName, occurrence]);
    const meta = test.meta().__errspecMutation;
    if (meta && (meta.runId !== runId || meta.requestId !== request.requestId))
      throw failure("EC_MUTATION_PROTOCOL", "Stale test metadata");
    const result = test.result();
    execution.tests.push({
      id,
      taskId: test.id,
      file: moduleFile,
      name: test.fullName,
      status: result.state,
      started: request.started.has(test.id),
      duration: test.diagnostic()?.duration || 0,
      errors: (result.errors || []).map((e) => `${e.name || "Error"}: ${e.message || e}`),
      hits: meta?.hits || {},
      infected: Object.keys(meta?.infected || {})
    });
  }
  const meta = module.meta().__errspecMutation;
  if (!meta && execution.tests.some((t) => t.file === moduleFile && t.started))
    throw failure("EC_MUTATION_PROTOCOL", "Test worker did not install the mutation runtime");
  if (meta) {
    if (options.weakMutation && meta.weakVersion !== 1)
      throw failure("EC_MUTATION_PROTOCOL", "Worker does not support weak mutation");
    if (meta.runId !== runId || meta.requestId !== request.requestId)
      throw failure("EC_MUTATION_PROTOCOL", "Stale module metadata");
    for (const fact of meta.childProcesses || []) {
      const entries = childProcessEntries(fact);
      if (!entries.length)
        continue;
      const test = execution.tests.find((t) => t.file === moduleFile && t.taskId === fact.taskId && t.started)?.id || null;
      (execution.childProcesses ||= []).push({ test, file: moduleFile, executable: basename(fact.argv[0]), entries, method: fact.method });
    }
    for (const [key, count] of Object.entries(meta.hits))
      execution.hits[key] = (execution.hits[key] || 0) + count;
    for (const key of Object.keys(meta.staticHits))
      (execution.staticHits[key] ||= []).push(moduleFile);
  }
  if (module.errors().length)
    execution.failedModules.push(moduleFile);
  for (const e of module.errors())
    execution.errors.push(`${e.name || "Error"}: ${e.message || e}`);
  for (const suite of module.children.allSuites())
    for (const e of suite.errors()) {
      if (!execution.failedModules.includes(moduleFile))
        execution.failedModules.push(moduleFile);
      execution.errors.push(`${e.name || "Error"}: ${e.message || e}`);
    }
}
async function closeContext(ctx) {
  process.exitCode = 0;
  await ctx.close();
  const failed = process.exitCode;
  process.exitCode = 0;
  if (failed)
    throw failure("EC_MUTATION_RUNNER", "Vitest cleanup reported a failure; see stderr");
}
async function execute(phase, request) {
  verifyInputs();
  if (phase === "plain" && warmContext) {
    const saved = warmContext;
    warmContext = undefined;
    await closeContext(saved.ctx);
  }
  infrastructureFailure = undefined;
  const execution = { selected: request.selected || [], tests: [], failedModules: [], errors: [], staticHits: {}, hits: {}, wallTimeout: false, moduleDurations: {} };
  request.started = new Set;
  const previous = phase !== "plain" ? warmContext : undefined;
  if (previous?.state.failure)
    throw previous.state.failure;
  warmContext = undefined;
  const state = previous?.state || {};
  Object.assign(state, { request, execution, active: true });
  const active = () => {
    if (!state.active) {
      state.failure = failure("EC_MUTATION_PROTOCOL", "Runner emitted an event outside its execution request");
      return false;
    }
    return true;
  };
  let ctx, completed = false, reusable = false, timeouts = previous?.timeouts;
  const report = {
    onTestCaseReady(test) {
      if (!active())
        return;
      if (!test.diagnostic()?.startTime)
        return;
      state.request.started.add(test.id);
      send(state.request, "progress", { event: "testStart" });
    },
    onTestModuleEnd(module) {
      if (active())
        try {
          recordModule(module, state.execution, state.request);
        } catch (error) {
          state.failure = error;
        }
    },
    onTestRunEnd(_modules, errors) {
      if (active())
        for (const e of errors)
          state.execution.errors.push(`${e.name || "Error"}: ${e.message || e}`);
    },
    onUserConsoleLog(log) {
      process.stderr.write(log.content);
      return false;
    }
  };
  try {
    ctx = previous?.ctx || await api.createVitest({
      root,
      config: options.vitestConfig || undefined,
      project: options.vitestProject ? [options.vitestProject] : undefined,
      watch: false,
      update: false,
      coverage: { enabled: false },
      reporters: [report],
      pool: "forks",
      isolate: true,
      maxWorkers: 1,
      fileParallelism: false,
      typecheck: { enabled: false, only: false },
      cache: false,
      fsModuleCache: false,
      allowOnly: false,
      passWithNoTests: false
    }, { plugins: [plugin(phase)], server: { fs: { allow: [root, options.temp, packageDir] } } });
    current = ctx;
    const projects = ctx.projects.filter((p) => !p.config.typecheck?.only);
    if (projects.length !== 1)
      throw failure("EC_MUTATION_CONFIG", `Select exactly one Vitest project with --vitest-project: ${projects.map((p) => p.config.name || "<unnamed>").join(", ")}`);
    const project = projects[0];
    projectName = project.config.name || "";
    if (project.config.browser?.enabled)
      throw failure("EC_MUTATION_CONFIG", "Vitest Browser Mode is not supported by mutate");
    const config = project.config;
    const rootProject = typeof ctx.getRootProject === "function" ? ctx.getRootProject() : undefined;
    if (options.reuseSession && phase !== "plain") {
      const globalSetup = [...ctx.projects, ...rootProject ? [rootProject] : []].some((p) => p.config.globalSetup?.length);
      reuseMode = globalSetup ? "fresh_global_setup" : rootProject && typeof ctx.clearSpecificationsCache === "function" ? "available" : "fresh_lifecycle_unknown";
      reusable = reuseMode === "available";
    }
    timeouts ??= { test: config.testTimeout, hook: config.hookTimeout };
    if (options.cache) {
      try {
        if ([...ctx.projects, ...rootProject ? [rootProject] : []].some((p) => p.config.globalSetup?.length))
          throw new Error("globalSetup may provide external inputs");
        const provided = { ...project.getProvidedContext() };
        delete provided.__errspecMutation;
        const sequence = { ...config.sequence };
        delete sequence.sequencer;
        if (!sequence.shuffle)
          delete sequence.seed;
        if (!mutationPlugins.cacheable)
          throw new Error("Plugin has no deterministic input contract");
        configurationHash = hash(serialize({
          compiler: compilerInputs(project.vite, config, relocatedPath),
          provided,
          env: config.env,
          include: config.include,
          exclude: config.exclude,
          testNamePattern: config.testNamePattern,
          testTimeout: timeouts.test,
          hookTimeout: timeouts.hook,
          retry: config.retry,
          bail: config.bail,
          sequence,
          setupFiles: config.setupFiles.filter((f) => f !== options.setup).map(relocatedPath),
          globalSetup: config.globalSetup,
          environment: config.environment,
          environmentOptions: config.environmentOptions,
          restoreMocks: config.restoreMocks,
          clearMocks: config.clearMocks,
          mockReset: config.mockReset,
          unstubEnvs: config.unstubEnvs,
          unstubGlobals: config.unstubGlobals,
          expect: config.expect,
          fakeTimers: config.fakeTimers,
          chaiConfig: config.chaiConfig
        }));
      } catch {
        configurationHash = null;
      }
    }
    config.watch = false;
    config.update = false;
    config.snapshotOptions = { ...config.snapshotOptions, updateSnapshot: "none" };
    ctx.config.snapshotOptions.updateSnapshot = "none";
    ctx.snapshot.options.updateSnapshot = "none";
    config.pool = "forks";
    config.isolate = true;
    config.maxWorkers = 1;
    config.fileParallelism = false;
    config.fsModuleCache = false;
    config.typecheck = { ...config.typecheck, enabled: false, only: false };
    config.coverage = { ...config.coverage, enabled: false };
    config.setupFiles = [options.setup, ...config.setupFiles.filter((f) => f !== options.setup)];
    config.provide = { ...config.provide, __errspecMutation: {
      runId,
      requestId: request.requestId,
      activeKey: request.key || null,
      hitLimit: request.hitLimit || Number.MAX_SAFE_INTEGER,
      weakMutation: options.weakMutation
    } };
    project.provide("__errspecMutation", config.provide.__errspecMutation);
    config.testTimeout = timeouts.test * (request.multiplier || 1);
    config.hookTimeout = timeouts.hook * (request.multiplier || 1);
    if (!previous)
      await ctx.standalone();
    if (typeof ctx.clearSpecificationsCache === "function")
      ctx.clearSpecificationsCache();
    const specs = await ctx.globTestSpecifications();
    const excluded = new Set([...mutationPlugins.files, ...configFiles, ...config.setupFiles, ...config.globalSetup || [], ...specs.map((s) => s.moduleId)].filter((f) => existsSync3(f)).map((f) => realpathSync3(f)));
    for (const file of globSync("**/*.{test,spec}.{ts,tsx,mts,cts,js,jsx,mjs,cjs}", { cwd: root, exclude: ["**/node_modules/**"] }))
      excluded.add(realpathSync3(resolve3(root, file)));
    for (const file of excluded)
      remember(file);
    if (phase === "plain") {
      const files = candidateFiles(excluded);
      for (const file of files) {
        const source = readFileSync3(file, "utf8");
        inventory.set(file, { source, candidates: [] });
        remember(file);
      }
      testSpecs = specIdentities(specs);
      for (const file of [...globSync("**/*.{test,spec}.{ts,tsx,mts,cts,js,jsx,mjs,cjs}", { cwd: root, exclude: ["**/node_modules/**"] })])
        remember(resolve3(root, file));
      for (const file of globSync("**/*.snap", { cwd: root, exclude: ["**/node_modules/**"] }))
        remember(resolve3(root, file));
      for (let dir = root;; dir = dirname3(dir)) {
        for (const name of ["package.json", "package-lock.json", "bun.lock", "bun.lockb", "pnpm-lock.yaml", "yarn.lock", "tsconfig.json"])
          remember(join3(dir, name));
        if (dirname3(dir) === dir)
          break;
      }
      if (options.plugins?.length)
        for (const [file, entry] of inventory)
          entry.generated = await generateSource(entry.source, rel(file));
      for (const file of mutationPlugins.files)
        inventory.delete(file);
    } else {
      const currentSpecs = specIdentities(specs);
      if (JSON.stringify(currentSpecs) !== JSON.stringify(testSpecs) || JSON.stringify(candidateFiles(excluded)) !== JSON.stringify([...inventory.keys()]))
        throw failure("EC_MUTATION_INPUT", "Source or test file inventory changed during the run");
    }
    let planned = specs;
    if (execution.selected.length) {
      const grouped = new Map;
      for (const id of execution.selected) {
        const base = baseline.tests.find((t) => t.id === id);
        if (!base)
          throw failure("EC_MUTATION_PROTOCOL", `Unknown test ID: ${id}`);
        const file = resolve3(root, base.file);
        (grouped.get(file) || grouped.set(file, []).get(file)).push(base.taskId);
      }
      planned = specs.filter((s) => grouped.has(s.moduleId));
      for (const spec of planned)
        spec.testIds = grouped.get(spec.moduleId);
    }
    if (phase !== "inspect")
      await ctx.runTestSpecifications(planned, true);
    if (state.failure)
      throw state.failure;
    if (infrastructureFailure)
      throw infrastructureFailure;
    if (phase === "plain" && options.experimentalCallgraph && !execution.failedModules.length && !execution.errors.length && execution.tests.some((test) => test.started && test.status === "passed") && !execution.tests.some((test) => test.status === "failed"))
      await acquireStaticImports(project);
    for (const project of ctx.projects)
      for (const environment of Object.values(project.vite.environments))
        for (const module of environment.moduleGraph?.idToModuleMap?.values() || []) {
          const file = module.file;
          if (file && !file.includes("/node_modules/") && !file.startsWith(options.temp) && existsSync3(file))
            remember(file);
        }
    verifyInputs();
    execution.tests.sort((a, b) => a.file < b.file ? -1 : a.file > b.file ? 1 : 0);
    completed = true;
    return execution;
  } catch (error) {
    if (!ctx && !error.code?.startsWith("EC_MUTATION_"))
      throw failure(phase === "plain" ? "EC_MUTATION_CONFIG" : "EC_MUTATION_RUNNER", error.stack || String(error));
    throw error;
  } finally {
    state.active = false;
    if (ctx && completed && reusable && !state.failure)
      warmContext = { ctx, state, timeouts };
    else if (ctx)
      await closeContext(ctx);
    current = undefined;
    process.exitCode = 0;
  }
}
function send(request, type, payload = {}) {
  protocolWrite(JSON.stringify({ version: PROTOCOL, runId, requestId: request.requestId, type, ...payload }) + `
`);
}
function cacheFacts() {
  return { version: 1, inputs: [...fingerprints], runnerPackage: packageDir, runtime: { node: process.versions.node, platform: process.platform, arch: process.arch, executable: process.execPath, cwd: process.cwd(), execArgv: process.execArgv, configurationHash, envHash: hash(JSON.stringify(Object.entries(process.env).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0).map(([name, value]) => [name, options.cacheRelocation && ["PWD", "INIT_CWD"].includes(name) ? relocatedPath(value) : value]))) } };
}
async function acquireStaticImports(project) {
  const container = project.vite.environments?.__vitest__?.pluginContainer ?? project.vite.environments?.ssr?.pluginContainer;
  for (const [file, entry] of inventory) {
    const generated = entry.generated ?? await generateSource(entry.source, rel(file), native(entry.source, rel(file)));
    pendingGenerated.set(file, generated);
    for (const binding of generated.metadata?.imports || []) {
      binding.resolved = null;
      try {
        const resolved = await container?.resolveId(binding.source, file);
        let target = resolved?.id?.split("?")[0];
        if (target?.startsWith("/@fs/"))
          target = target.slice(4);
        if (!target || !isAbsolute2(target) || !existsSync3(target))
          continue;
        target = normalize(realpathSync3(target));
        remember(target);
        if (inventory.has(target))
          binding.resolved = rel(target);
      } catch {}
    }
  }
}
async function handle(request) {
  if (request.version !== PROTOCOL || typeof request.runId !== "string" || !Number.isSafeInteger(request.requestId))
    throw failure("EC_MUTATION_PROTOCOL", "Invalid request envelope");
  if (runId && runId !== request.runId)
    throw failure("EC_MUTATION_PROTOCOL", "Wrong run ID");
  runId = request.runId;
  if (request.op === "open") {
    options = request.options;
    pendingGenerated.clear();
    if (Number(process.versions.node.split(".")[0]) < 24)
      throw failure("EC_MUTATION_CONFIG", "mutate requires Node 24 or newer");
    root = realpathSync3(options.root);
    toolTemp = realpathSync3(options.toolTemp || options.temp);
    packageDir = packagePath(options.vitestPackage);
    mutationPlugins = await loadPlugins(options.plugins, root, remember, options.excludeMutator);
    const pkg = JSON.parse(readFileSync3(join3(packageDir, "package.json"), "utf8"));
    remember(join3(packageDir, "package.json"));
    if (!/^5\./.test(pkg.version))
      throw failure("EC_MUTATION_CONFIG", `mutate requires Vitest 5.x; found ${pkg.version}`);
    api = await import(pathToFileURL3(join3(packageDir, "dist/node.js")).href);
    const setup = readFileSync3(options.setup, "utf8").replace('"__VITEST_ENTRY__"', JSON.stringify(normalize(join3(packageDir, "dist/index.js"))));
    if (setup !== readFileSync3(options.setup, "utf8"))
      writeFileSync(options.setup, setup);
    if (request.restore) {
      const saved = JSON.parse(readFileSync3(options.state, "utf8"));
      baseline = saved.baseline;
      plain = saved.plain;
      testSpecs = saved.testSpecs;
      projectName = saved.projectName;
      const restoredInputs = new Map(saved.fingerprints);
      for (const [file, digest] of fingerprints)
        if (restoredInputs.get(file) !== digest)
          throw failure("EC_MUTATION_INPUT", `Input closure changed while restoring worker: ${file}`);
      inventory = new Map(saved.inventory);
      fingerprints = restoredInputs;
      verifyInputs();
      for (const [file, entry] of inventory)
        entry.emitted = instrument(entry.source, entry.candidates, file, runId);
    }
    send(request, "done", { node: process.versions.node, vitest: pkg.version, mutators: mutationPlugins.names, count: 0 });
    return;
  }
  if (!options)
    throw failure("EC_MUTATION_PROTOCOL", "Driver was not opened");
  if (request.op === "prime") {
    if (!baseline)
      throw failure("EC_MUTATION_PROTOCOL", "Missing restored worker baseline");
    const measured = await execute("probe", request);
    baseline = measured;
    send(request, "primed", { execution: measured, ...options.cache ? { cacheFacts: cacheFacts() } : {} });
    send(request, "done", { count: 0 });
    return;
  }
  if (request.op === "cache-facts") {
    if (!options.cache)
      throw failure("EC_MUTATION_PROTOCOL", "Result caching was not requested");
    verifyInputs();
    send(request, "done", { count: 0, cacheFacts: cacheFacts() });
    return;
  }
  if (request.op === "prepare" || request.op === "discover") {
    plain = await execute("plain", request);
    send(request, "baseline", { execution: plain });
    if (plain.failedModules.length || plain.errors.length || !plain.tests.some((t) => t.started && t.status === "passed") || plain.tests.some((t) => t.status === "failed"))
      throw failure("EC_MUTATION_BASELINE", "Unmodified baseline failed or executed no tests");
    let count = 0;
    for (const [file, entry] of inventory) {
      const generated = pendingGenerated.get(file) ?? entry.generated ?? await generateSource(entry.source, rel(file));
      pendingGenerated.delete(file);
      delete entry.generated;
      entry.candidates = generated.candidates;
      entry.critical = generated.critical || [];
      for (const c of entry.candidates)
        if (c.regex) {
          try {
            new RegExp(...c.regex);
          } catch {
            c.ignored = "Regex replacement is invalid for the original flags";
          }
        }
      entry.emitted = instrument(entry.source, entry.candidates, file, runId);
      try {
        native(entry.emitted.code, rel(file), true);
      } catch (error) {
        for (const c of entry.candidates.filter((c) => !c.ignored)) {
          try {
            native(instrument(entry.source, [c], file, runId).code, rel(file), true);
          } catch {
            c.ignored = "Switch placement is not valid in this syntax context";
          }
        }
        entry.emitted = instrument(entry.source, entry.candidates, file, runId);
        native(entry.emitted.code, rel(file), true);
      }
      if (generated.metadata) {
        for (const scope of Object.values(generated.metadata.scopes))
          if (scope.entryKey && !entry.candidates.some((c) => c.key === scope.entryKey && !c.ignored))
            delete scope.entryKey;
      }
      const critical = options.policy?.critical.some((pattern) => matchesGlob(rel(file), pattern)) ? entry.candidates.map((c) => c.key) : entry.critical;
      send(request, "file", { file: rel(file), candidates: entry.candidates, critical, ...generated.metadata ? { metadata: generated.metadata } : {} });
      count++;
    }
    if (request.op === "discover") {
      send(request, "discovered", { cacheFacts: cacheFacts() });
      send(request, "done", { count });
      return;
    }
    await prepareProbe(request, count);
    return;
  }
  if (request.op === "probe") {
    if (!plain || baseline)
      throw failure("EC_MUTATION_PROTOCOL", "Probe requires an unprepared current baseline");
    await prepareProbe(request, 0);
    return;
  }
  if (request.op === "run") {
    if (!baseline)
      throw failure("EC_MUTATION_PROTOCOL", "Missing baseline");
    if (![...inventory.values()].some((entry) => entry.candidates.some((c) => c.key === request.key && !c.ignored)))
      throw failure("EC_MUTATION_PROTOCOL", "Unknown or ignored active mutant key");
    const execution = await execute("mutant", request);
    send(request, "attempt", { key: request.key, execution });
    send(request, "done", { count: 1 });
    return;
  }
  if (request.op === "typecheck") {
    if (!baseline)
      throw failure("EC_MUTATION_PROTOCOL", "Missing baseline");
    verifyInputs();
    try {
      const keys = new Set(request.keys);
      const selected = new Map([...inventory].map(([file, entry]) => [file, { ...entry, candidates: entry.candidates.filter((c) => keys.has(c.key)) }]));
      const result = await typecheckMutants(options, selected, (key) => send(request, "progress", { event: "typecheck", key }));
      verifyInputs();
      send(request, "typechecked", result);
      send(request, "done", { count: result.outcomes.length });
      return;
    } catch (error) {
      throw failure("EC_MUTATION_TYPECHECK", error.stack || String(error));
    }
  }
  if (request.op === "close") {
    verifyInputs();
    if ((options.cache || options.concurrency > 1) && baseline)
      await execute("inspect", request);
    if (warmContext) {
      const saved = warmContext;
      warmContext = undefined;
      await closeContext(saved.ctx);
      if (saved.state.failure)
        throw saved.state.failure;
    }
    verifyInputs();
    send(request, "done", { count: 0, ...options.cache ? { cacheFacts: cacheFacts() } : {} });
    return;
  }
  throw failure("EC_MUTATION_PROTOCOL", `Unknown operation: ${request.op}`);
}
async function prepareProbe(request, count) {
  if (request.coverage) {
    baseline = request.coverage;
    await execute("inspect", request);
  } else
    baseline = await execute("probe", request);
  const identity = (e) => e.tests.map((t) => [t.id, t.status, t.started]);
  if (JSON.stringify(identity(plain)) !== JSON.stringify(identity(baseline)) || baseline.errors.length || baseline.failedModules.length)
    throw failure("EC_MUTATION_INSTRUMENTATION", "Probe baseline changed test registration or results");
  writeFileSync(options.state, JSON.stringify({ baseline, plain, testSpecs, projectName, inventory: [...inventory], fingerprints: [...fingerprints] }));
  send(request, "prepared", { execution: baseline, ...options.reuseSession ? { sessionReuse: reuseMode } : {}, ...options.cache ? { cacheFacts: cacheFacts() } : {} });
  send(request, "done", { count });
  return;
}
var lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of lines) {
  let request;
  try {
    request = JSON.parse(line);
    await handle(request);
  } catch (error) {
    try {
      if (current)
        await closeContext(current);
      if (warmContext) {
        const saved = warmContext;
        warmContext = undefined;
        await closeContext(saved.ctx);
      }
    } catch (cleanup) {
      error = failure(error.code || "EC_MUTATION_RUNNER", `${error.stack || error}
Cleanup failed: ${cleanup.stack || cleanup}`);
    }
    send(request || { requestId: 0 }, "error", { code: error.code?.startsWith("EC_MUTATION_") ? error.code : "EC_MUTATION_RUNNER", message: error.stack || String(error) });
    process.exitCode = 0;
  }
}
if (warmContext)
  await closeContext(warmContext.ctx);
