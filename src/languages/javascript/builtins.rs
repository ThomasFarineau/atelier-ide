//! What JavaScript and Node provide without an import: globals, the members of the built-in
//! types, keywords and code snippets. The type names are the keys of [`members`].

use crate::languages::{CompletionItem, ItemKind};

/// A member of a built-in type: (name, signature, documentation, type of the value or result).
/// The documentation is a key of the locales (`javascript.string.at`), as in the other tables.
/// The signature starts with `(` for a method; a property has the type only.
pub type Member = (&'static str, &'static str, &'static str, &'static str);

pub const STRING: &[Member] = &[
    ("length", "number", "javascript.string.length", "number"),
    (
        "at",
        "(index: number): string",
        "javascript.string.at",
        "string",
    ),
    (
        "charAt",
        "(index: number): string",
        "javascript.string.charAt",
        "string",
    ),
    (
        "charCodeAt",
        "(index: number): number",
        "javascript.string.charCodeAt",
        "number",
    ),
    (
        "codePointAt",
        "(index: number): number",
        "javascript.string.codePointAt",
        "number",
    ),
    (
        "concat",
        "(...strings: string[]): string",
        "javascript.string.concat",
        "string",
    ),
    (
        "endsWith",
        "(search: string, length?: number): boolean",
        "javascript.string.endsWith",
        "boolean",
    ),
    (
        "includes",
        "(search: string, from?: number): boolean",
        "javascript.string.includes",
        "boolean",
    ),
    (
        "indexOf",
        "(search: string, from?: number): number",
        "javascript.string.indexOf",
        "number",
    ),
    (
        "lastIndexOf",
        "(search: string, from?: number): number",
        "javascript.string.lastIndexOf",
        "number",
    ),
    (
        "localeCompare",
        "(other: string): number",
        "javascript.string.localeCompare",
        "number",
    ),
    (
        "match",
        "(regexp: RegExp | string): RegExpMatchArray | null",
        "javascript.string.match",
        "Array",
    ),
    (
        "matchAll",
        "(regexp: RegExp): Iterator",
        "javascript.string.matchAll",
        "Array",
    ),
    (
        "normalize",
        "(form?: string): string",
        "javascript.string.normalize",
        "string",
    ),
    (
        "padEnd",
        "(length: number, fill?: string): string",
        "javascript.string.padEnd",
        "string",
    ),
    (
        "padStart",
        "(length: number, fill?: string): string",
        "javascript.string.padStart",
        "string",
    ),
    (
        "repeat",
        "(count: number): string",
        "javascript.string.repeat",
        "string",
    ),
    (
        "replace",
        "(search: string | RegExp, by: string | Function): string",
        "javascript.string.replace",
        "string",
    ),
    (
        "replaceAll",
        "(search: string | RegExp, by: string | Function): string",
        "javascript.string.replaceAll",
        "string",
    ),
    (
        "search",
        "(regexp: RegExp): number",
        "javascript.string.search",
        "number",
    ),
    (
        "slice",
        "(start?: number, end?: number): string",
        "javascript.string.slice",
        "string",
    ),
    (
        "split",
        "(separator: string | RegExp, limit?: number): string[]",
        "javascript.string.split",
        "Array",
    ),
    (
        "startsWith",
        "(search: string, from?: number): boolean",
        "javascript.string.startsWith",
        "boolean",
    ),
    (
        "substring",
        "(start: number, end?: number): string",
        "javascript.string.substring",
        "string",
    ),
    (
        "toLocaleLowerCase",
        "(locale?: string): string",
        "javascript.string.toLocaleLowerCase",
        "string",
    ),
    (
        "toLocaleUpperCase",
        "(locale?: string): string",
        "javascript.string.toLocaleUpperCase",
        "string",
    ),
    (
        "toLowerCase",
        "(): string",
        "javascript.string.toLowerCase",
        "string",
    ),
    (
        "toString",
        "(): string",
        "javascript.string.toString",
        "string",
    ),
    (
        "toUpperCase",
        "(): string",
        "javascript.string.toUpperCase",
        "string",
    ),
    ("trim", "(): string", "javascript.string.trim", "string"),
    (
        "trimEnd",
        "(): string",
        "javascript.string.trimEnd",
        "string",
    ),
    (
        "trimStart",
        "(): string",
        "javascript.string.trimStart",
        "string",
    ),
    (
        "valueOf",
        "(): string",
        "javascript.string.valueOf",
        "string",
    ),
];

pub const ARRAY: &[Member] = &[
    ("length", "number", "javascript.array.length", "number"),
    ("at", "(index: number): T", "javascript.array.at", "any"),
    (
        "concat",
        "(...items: T[]): T[]",
        "javascript.array.concat",
        "Array",
    ),
    (
        "copyWithin",
        "(target: number, start: number, end?: number): T[]",
        "javascript.array.copyWithin",
        "Array",
    ),
    (
        "entries",
        "(): Iterator<[number, T]>",
        "javascript.array.entries",
        "Array",
    ),
    (
        "every",
        "(test: (value: T, index: number) => boolean): boolean",
        "javascript.array.every",
        "boolean",
    ),
    (
        "fill",
        "(value: T, start?: number, end?: number): T[]",
        "javascript.array.fill",
        "Array",
    ),
    (
        "filter",
        "(test: (value: T, index: number) => boolean): T[]",
        "javascript.array.filter",
        "Array",
    ),
    (
        "find",
        "(test: (value: T, index: number) => boolean): T | undefined",
        "javascript.array.find",
        "any",
    ),
    (
        "findIndex",
        "(test: (value: T, index: number) => boolean): number",
        "javascript.array.findIndex",
        "number",
    ),
    (
        "findLast",
        "(test: (value: T, index: number) => boolean): T | undefined",
        "javascript.array.findLast",
        "any",
    ),
    (
        "findLastIndex",
        "(test: (value: T, index: number) => boolean): number",
        "javascript.array.findLastIndex",
        "number",
    ),
    (
        "flat",
        "(depth?: number): any[]",
        "javascript.array.flat",
        "Array",
    ),
    (
        "flatMap",
        "(map: (value: T, index: number) => U | U[]): U[]",
        "javascript.array.flatMap",
        "Array",
    ),
    (
        "forEach",
        "(action: (value: T, index: number) => void): void",
        "javascript.array.forEach",
        "void",
    ),
    (
        "includes",
        "(value: T, from?: number): boolean",
        "javascript.array.includes",
        "boolean",
    ),
    (
        "indexOf",
        "(value: T, from?: number): number",
        "javascript.array.indexOf",
        "number",
    ),
    (
        "join",
        "(separator?: string): string",
        "javascript.array.join",
        "string",
    ),
    (
        "keys",
        "(): Iterator<number>",
        "javascript.array.keys",
        "Array",
    ),
    (
        "lastIndexOf",
        "(value: T, from?: number): number",
        "javascript.array.lastIndexOf",
        "number",
    ),
    (
        "map",
        "(map: (value: T, index: number) => U): U[]",
        "javascript.array.map",
        "Array",
    ),
    ("pop", "(): T | undefined", "javascript.array.pop", "any"),
    (
        "push",
        "(...items: T[]): number",
        "javascript.array.push",
        "number",
    ),
    (
        "reduce",
        "(reducer: (acc: U, value: T) => U, initial?: U): U",
        "javascript.array.reduce",
        "any",
    ),
    (
        "reduceRight",
        "(reducer: (acc: U, value: T) => U, initial?: U): U",
        "javascript.array.reduceRight",
        "any",
    ),
    ("reverse", "(): T[]", "javascript.array.reverse", "Array"),
    (
        "shift",
        "(): T | undefined",
        "javascript.array.shift",
        "any",
    ),
    (
        "slice",
        "(start?: number, end?: number): T[]",
        "javascript.array.slice",
        "Array",
    ),
    (
        "some",
        "(test: (value: T, index: number) => boolean): boolean",
        "javascript.array.some",
        "boolean",
    ),
    (
        "sort",
        "(compare?: (a: T, b: T) => number): T[]",
        "javascript.array.sort",
        "Array",
    ),
    (
        "splice",
        "(start: number, deleteCount?: number, ...items: T[]): T[]",
        "javascript.array.splice",
        "Array",
    ),
    (
        "toReversed",
        "(): T[]",
        "javascript.array.toReversed",
        "Array",
    ),
    (
        "toSorted",
        "(compare?: (a: T, b: T) => number): T[]",
        "javascript.array.toSorted",
        "Array",
    ),
    (
        "toSpliced",
        "(start: number, deleteCount?: number, ...items: T[]): T[]",
        "javascript.array.toSpliced",
        "Array",
    ),
    (
        "toString",
        "(): string",
        "javascript.array.toString",
        "string",
    ),
    (
        "unshift",
        "(...items: T[]): number",
        "javascript.array.unshift",
        "number",
    ),
    (
        "values",
        "(): Iterator<T>",
        "javascript.array.values",
        "Array",
    ),
    (
        "with",
        "(index: number, value: T): T[]",
        "javascript.array.with",
        "Array",
    ),
];

pub const NUMBER: &[Member] = &[
    (
        "toExponential",
        "(digits?: number): string",
        "javascript.number.toExponential",
        "string",
    ),
    (
        "toFixed",
        "(digits?: number): string",
        "javascript.number.toFixed",
        "string",
    ),
    (
        "toLocaleString",
        "(locale?: string, options?: object): string",
        "javascript.number.toLocaleString",
        "string",
    ),
    (
        "toPrecision",
        "(precision?: number): string",
        "javascript.number.toPrecision",
        "string",
    ),
    (
        "toString",
        "(radix?: number): string",
        "javascript.number.toString",
        "string",
    ),
    (
        "valueOf",
        "(): number",
        "javascript.number.valueOf",
        "number",
    ),
];

pub const BOOLEAN: &[Member] = &[
    (
        "toString",
        "(): string",
        "javascript.boolean.toString",
        "string",
    ),
    (
        "valueOf",
        "(): boolean",
        "javascript.boolean.valueOf",
        "boolean",
    ),
];

pub const FUNCTION: &[Member] = &[
    (
        "apply",
        "(thisArg: any, args?: any[]): any",
        "javascript.function.apply",
        "any",
    ),
    (
        "bind",
        "(thisArg: any, ...args: any[]): Function",
        "javascript.function.bind",
        "Function",
    ),
    (
        "call",
        "(thisArg: any, ...args: any[]): any",
        "javascript.function.call",
        "any",
    ),
    ("length", "number", "javascript.function.length", "number"),
    ("name", "string", "javascript.function.name", "string"),
    (
        "toString",
        "(): string",
        "javascript.function.toString",
        "string",
    ),
];

pub const OBJECT: &[Member] = &[
    (
        "constructor",
        "Function",
        "javascript.object.constructor",
        "Function",
    ),
    (
        "hasOwnProperty",
        "(key: string): boolean",
        "javascript.object.hasOwnProperty",
        "boolean",
    ),
    (
        "isPrototypeOf",
        "(object: object): boolean",
        "javascript.object.isPrototypeOf",
        "boolean",
    ),
    (
        "propertyIsEnumerable",
        "(key: string): boolean",
        "javascript.object.propertyIsEnumerable",
        "boolean",
    ),
    (
        "toLocaleString",
        "(): string",
        "javascript.object.toLocaleString",
        "string",
    ),
    (
        "toString",
        "(): string",
        "javascript.object.toString",
        "string",
    ),
    ("valueOf", "(): any", "javascript.object.valueOf", "any"),
];

pub const OBJECT_STATIC: &[Member] = &[
    (
        "assign",
        "(target: object, ...sources: object[]): object",
        "javascript.object_static.assign",
        "Object",
    ),
    (
        "create",
        "(prototype: object | null, properties?: object): object",
        "javascript.object_static.create",
        "Object",
    ),
    (
        "defineProperties",
        "(object: object, properties: object): object",
        "javascript.object_static.defineProperties",
        "Object",
    ),
    (
        "defineProperty",
        "(object: object, key: string | symbol, descriptor: object): object",
        "javascript.object_static.defineProperty",
        "Object",
    ),
    (
        "entries",
        "(object: object): [string, any][]",
        "javascript.object_static.entries",
        "Array",
    ),
    (
        "freeze",
        "(object: object): object",
        "javascript.object_static.freeze",
        "Object",
    ),
    (
        "fromEntries",
        "(entries: Iterable<[string, any]>): object",
        "javascript.object_static.fromEntries",
        "Object",
    ),
    (
        "getOwnPropertyDescriptor",
        "(object: object, key: string): object",
        "javascript.object_static.getOwnPropertyDescriptor",
        "Object",
    ),
    (
        "getOwnPropertyDescriptors",
        "(object: object): object",
        "javascript.object_static.getOwnPropertyDescriptors",
        "Object",
    ),
    (
        "getOwnPropertyNames",
        "(object: object): string[]",
        "javascript.object_static.getOwnPropertyNames",
        "Array",
    ),
    (
        "getOwnPropertySymbols",
        "(object: object): symbol[]",
        "javascript.object_static.getOwnPropertySymbols",
        "Array",
    ),
    (
        "getPrototypeOf",
        "(object: object): object | null",
        "javascript.object_static.getPrototypeOf",
        "Object",
    ),
    (
        "groupBy",
        "(items: Iterable<T>, key: (item: T) => string): object",
        "javascript.object_static.groupBy",
        "Object",
    ),
    (
        "hasOwn",
        "(object: object, key: string): boolean",
        "javascript.object_static.hasOwn",
        "boolean",
    ),
    (
        "is",
        "(a: any, b: any): boolean",
        "javascript.object_static.is",
        "boolean",
    ),
    (
        "isExtensible",
        "(object: object): boolean",
        "javascript.object_static.isExtensible",
        "boolean",
    ),
    (
        "isFrozen",
        "(object: object): boolean",
        "javascript.object_static.isFrozen",
        "boolean",
    ),
    (
        "isSealed",
        "(object: object): boolean",
        "javascript.object_static.isSealed",
        "boolean",
    ),
    (
        "keys",
        "(object: object): string[]",
        "javascript.object_static.keys",
        "Array",
    ),
    (
        "preventExtensions",
        "(object: object): object",
        "javascript.object_static.preventExtensions",
        "Object",
    ),
    (
        "seal",
        "(object: object): object",
        "javascript.object_static.seal",
        "Object",
    ),
    (
        "setPrototypeOf",
        "(object: object, prototype: object | null): object",
        "javascript.object_static.setPrototypeOf",
        "Object",
    ),
    (
        "values",
        "(object: object): any[]",
        "javascript.object_static.values",
        "Array",
    ),
];

pub const ARRAY_STATIC: &[Member] = &[
    (
        "from",
        "(items: Iterable<T> | ArrayLike<T>, map?: (value: T, index: number) => U): U[]",
        "javascript.array_static.from",
        "Array",
    ),
    (
        "fromAsync",
        "(items: AsyncIterable<T>): Promise<T[]>",
        "javascript.array_static.fromAsync",
        "Promise",
    ),
    (
        "isArray",
        "(value: any): boolean",
        "javascript.array_static.isArray",
        "boolean",
    ),
    (
        "of",
        "(...items: T[]): T[]",
        "javascript.array_static.of",
        "Array",
    ),
];

pub const STRING_STATIC: &[Member] = &[
    (
        "fromCharCode",
        "(...codes: number[]): string",
        "javascript.string_static.fromCharCode",
        "string",
    ),
    (
        "fromCodePoint",
        "(...codes: number[]): string",
        "javascript.string_static.fromCodePoint",
        "string",
    ),
    (
        "raw",
        "(template: TemplateStringsArray, ...values: any[]): string",
        "javascript.string_static.raw",
        "string",
    ),
];

pub const NUMBER_STATIC: &[Member] = &[
    (
        "EPSILON",
        "number",
        "javascript.number_static.EPSILON",
        "number",
    ),
    (
        "MAX_SAFE_INTEGER",
        "number",
        "javascript.number_static.MAX_SAFE_INTEGER",
        "number",
    ),
    (
        "MAX_VALUE",
        "number",
        "javascript.number_static.MAX_VALUE",
        "number",
    ),
    (
        "MIN_SAFE_INTEGER",
        "number",
        "javascript.number_static.MIN_SAFE_INTEGER",
        "number",
    ),
    (
        "MIN_VALUE",
        "number",
        "javascript.number_static.MIN_VALUE",
        "number",
    ),
    ("NaN", "number", "javascript.number_static.NaN", "number"),
    (
        "NEGATIVE_INFINITY",
        "number",
        "javascript.number_static.NEGATIVE_INFINITY",
        "number",
    ),
    (
        "POSITIVE_INFINITY",
        "number",
        "javascript.number_static.POSITIVE_INFINITY",
        "number",
    ),
    (
        "isFinite",
        "(value: any): boolean",
        "javascript.number_static.isFinite",
        "boolean",
    ),
    (
        "isInteger",
        "(value: any): boolean",
        "javascript.number_static.isInteger",
        "boolean",
    ),
    (
        "isNaN",
        "(value: any): boolean",
        "javascript.number_static.isNaN",
        "boolean",
    ),
    (
        "isSafeInteger",
        "(value: any): boolean",
        "javascript.number_static.isSafeInteger",
        "boolean",
    ),
    (
        "parseFloat",
        "(text: string): number",
        "javascript.number_static.parseFloat",
        "number",
    ),
    (
        "parseInt",
        "(text: string, radix?: number): number",
        "javascript.number_static.parseInt",
        "number",
    ),
];

pub const MATH: &[Member] = &[
    ("E", "number", "javascript.math.E", "number"),
    ("LN10", "number", "javascript.math.LN10", "number"),
    ("LN2", "number", "javascript.math.LN2", "number"),
    ("LOG10E", "number", "javascript.math.LOG10E", "number"),
    ("LOG2E", "number", "javascript.math.LOG2E", "number"),
    ("PI", "number", "javascript.math.PI", "number"),
    ("SQRT1_2", "number", "javascript.math.SQRT1_2", "number"),
    ("SQRT2", "number", "javascript.math.SQRT2", "number"),
    (
        "abs",
        "(x: number): number",
        "javascript.math.abs",
        "number",
    ),
    (
        "acos",
        "(x: number): number",
        "javascript.math.acos",
        "number",
    ),
    (
        "asin",
        "(x: number): number",
        "javascript.math.asin",
        "number",
    ),
    (
        "atan",
        "(x: number): number",
        "javascript.math.atan",
        "number",
    ),
    (
        "atan2",
        "(y: number, x: number): number",
        "javascript.math.atan2",
        "number",
    ),
    (
        "cbrt",
        "(x: number): number",
        "javascript.math.cbrt",
        "number",
    ),
    (
        "ceil",
        "(x: number): number",
        "javascript.math.ceil",
        "number",
    ),
    (
        "clz32",
        "(x: number): number",
        "javascript.math.clz32",
        "number",
    ),
    (
        "cos",
        "(x: number): number",
        "javascript.math.cos",
        "number",
    ),
    (
        "exp",
        "(x: number): number",
        "javascript.math.exp",
        "number",
    ),
    (
        "floor",
        "(x: number): number",
        "javascript.math.floor",
        "number",
    ),
    (
        "fround",
        "(x: number): number",
        "javascript.math.fround",
        "number",
    ),
    (
        "hypot",
        "(...values: number[]): number",
        "javascript.math.hypot",
        "number",
    ),
    (
        "imul",
        "(a: number, b: number): number",
        "javascript.math.imul",
        "number",
    ),
    (
        "log",
        "(x: number): number",
        "javascript.math.log",
        "number",
    ),
    (
        "log10",
        "(x: number): number",
        "javascript.math.log10",
        "number",
    ),
    (
        "log1p",
        "(x: number): number",
        "javascript.math.log1p",
        "number",
    ),
    (
        "log2",
        "(x: number): number",
        "javascript.math.log2",
        "number",
    ),
    (
        "max",
        "(...values: number[]): number",
        "javascript.math.max",
        "number",
    ),
    (
        "min",
        "(...values: number[]): number",
        "javascript.math.min",
        "number",
    ),
    (
        "pow",
        "(base: number, exponent: number): number",
        "javascript.math.pow",
        "number",
    ),
    ("random", "(): number", "javascript.math.random", "number"),
    (
        "round",
        "(x: number): number",
        "javascript.math.round",
        "number",
    ),
    (
        "sign",
        "(x: number): number",
        "javascript.math.sign",
        "number",
    ),
    (
        "sin",
        "(x: number): number",
        "javascript.math.sin",
        "number",
    ),
    (
        "sqrt",
        "(x: number): number",
        "javascript.math.sqrt",
        "number",
    ),
    (
        "tan",
        "(x: number): number",
        "javascript.math.tan",
        "number",
    ),
    (
        "trunc",
        "(x: number): number",
        "javascript.math.trunc",
        "number",
    ),
];

pub const JSON: &[Member] = &[
    (
        "parse",
        "(text: string, reviver?: Function): any",
        "javascript.json.parse",
        "any",
    ),
    (
        "stringify",
        "(value: any, replacer?: Function | any[], space?: number | string): string",
        "javascript.json.stringify",
        "string",
    ),
];

pub const CONSOLE: &[Member] = &[
    (
        "assert",
        "(condition: boolean, ...data: any[]): void",
        "javascript.console.assert",
        "void",
    ),
    ("clear", "(): void", "javascript.console.clear", "void"),
    (
        "count",
        "(label?: string): void",
        "javascript.console.count",
        "void",
    ),
    (
        "debug",
        "(...data: any[]): void",
        "javascript.console.debug",
        "void",
    ),
    (
        "dir",
        "(object: any, options?: object): void",
        "javascript.console.dir",
        "void",
    ),
    (
        "error",
        "(...data: any[]): void",
        "javascript.console.error",
        "void",
    ),
    (
        "group",
        "(...label: any[]): void",
        "javascript.console.group",
        "void",
    ),
    (
        "groupEnd",
        "(): void",
        "javascript.console.groupEnd",
        "void",
    ),
    (
        "info",
        "(...data: any[]): void",
        "javascript.console.info",
        "void",
    ),
    (
        "log",
        "(...data: any[]): void",
        "javascript.console.log",
        "void",
    ),
    (
        "table",
        "(data: any, columns?: string[]): void",
        "javascript.console.table",
        "void",
    ),
    (
        "time",
        "(label?: string): void",
        "javascript.console.time",
        "void",
    ),
    (
        "timeEnd",
        "(label?: string): void",
        "javascript.console.timeEnd",
        "void",
    ),
    (
        "trace",
        "(...data: any[]): void",
        "javascript.console.trace",
        "void",
    ),
    (
        "warn",
        "(...data: any[]): void",
        "javascript.console.warn",
        "void",
    ),
];

pub const PROMISE: &[Member] = &[
    (
        "catch",
        "(onRejected: (reason: any) => any): Promise",
        "javascript.promise.catch",
        "Promise",
    ),
    (
        "finally",
        "(onFinally: () => void): Promise",
        "javascript.promise.finally",
        "Promise",
    ),
    (
        "then",
        "(onFulfilled: (value: T) => any, onRejected?: (reason: any) => any): Promise",
        "javascript.promise.then",
        "Promise",
    ),
];

pub const PROMISE_STATIC: &[Member] = &[
    (
        "all",
        "(promises: Iterable<Promise>): Promise<any[]>",
        "javascript.promise_static.all",
        "Promise",
    ),
    (
        "allSettled",
        "(promises: Iterable<Promise>): Promise<object[]>",
        "javascript.promise_static.allSettled",
        "Promise",
    ),
    (
        "any",
        "(promises: Iterable<Promise>): Promise",
        "javascript.promise_static.any",
        "Promise",
    ),
    (
        "race",
        "(promises: Iterable<Promise>): Promise",
        "javascript.promise_static.race",
        "Promise",
    ),
    (
        "reject",
        "(reason: any): Promise",
        "javascript.promise_static.reject",
        "Promise",
    ),
    (
        "resolve",
        "(value?: any): Promise",
        "javascript.promise_static.resolve",
        "Promise",
    ),
    (
        "withResolvers",
        "(): { promise, resolve, reject }",
        "javascript.promise_static.withResolvers",
        "Object",
    ),
];

pub const MAP: &[Member] = &[
    ("size", "number", "javascript.map.size", "number"),
    ("clear", "(): void", "javascript.map.clear", "void"),
    (
        "delete",
        "(key: K): boolean",
        "javascript.map.delete",
        "boolean",
    ),
    (
        "entries",
        "(): Iterator<[K, V]>",
        "javascript.map.entries",
        "Array",
    ),
    (
        "forEach",
        "(action: (value: V, key: K) => void): void",
        "javascript.map.forEach",
        "void",
    ),
    (
        "get",
        "(key: K): V | undefined",
        "javascript.map.get",
        "any",
    ),
    ("has", "(key: K): boolean", "javascript.map.has", "boolean"),
    ("keys", "(): Iterator<K>", "javascript.map.keys", "Array"),
    (
        "set",
        "(key: K, value: V): Map",
        "javascript.map.set",
        "Map",
    ),
    (
        "values",
        "(): Iterator<V>",
        "javascript.map.values",
        "Array",
    ),
];

pub const SET: &[Member] = &[
    ("size", "number", "javascript.set.size", "number"),
    ("add", "(value: T): Set", "javascript.set.add", "Set"),
    ("clear", "(): void", "javascript.set.clear", "void"),
    (
        "delete",
        "(value: T): boolean",
        "javascript.set.delete",
        "boolean",
    ),
    (
        "entries",
        "(): Iterator<[T, T]>",
        "javascript.set.entries",
        "Array",
    ),
    (
        "forEach",
        "(action: (value: T) => void): void",
        "javascript.set.forEach",
        "void",
    ),
    (
        "has",
        "(value: T): boolean",
        "javascript.set.has",
        "boolean",
    ),
    ("keys", "(): Iterator<T>", "javascript.set.keys", "Array"),
    (
        "values",
        "(): Iterator<T>",
        "javascript.set.values",
        "Array",
    ),
];

pub const DATE: &[Member] = &[
    ("getDate", "(): number", "javascript.date.getDate", "number"),
    ("getDay", "(): number", "javascript.date.getDay", "number"),
    (
        "getFullYear",
        "(): number",
        "javascript.date.getFullYear",
        "number",
    ),
    (
        "getHours",
        "(): number",
        "javascript.date.getHours",
        "number",
    ),
    (
        "getMilliseconds",
        "(): number",
        "javascript.date.getMilliseconds",
        "number",
    ),
    (
        "getMinutes",
        "(): number",
        "javascript.date.getMinutes",
        "number",
    ),
    (
        "getMonth",
        "(): number",
        "javascript.date.getMonth",
        "number",
    ),
    (
        "getSeconds",
        "(): number",
        "javascript.date.getSeconds",
        "number",
    ),
    ("getTime", "(): number", "javascript.date.getTime", "number"),
    (
        "getTimezoneOffset",
        "(): number",
        "javascript.date.getTimezoneOffset",
        "number",
    ),
    (
        "setDate",
        "(day: number): number",
        "javascript.date.setDate",
        "number",
    ),
    (
        "setFullYear",
        "(year: number, month?: number, day?: number): number",
        "javascript.date.setFullYear",
        "number",
    ),
    (
        "setHours",
        "(hours: number, min?: number, sec?: number, ms?: number): number",
        "javascript.date.setHours",
        "number",
    ),
    (
        "setMinutes",
        "(min: number, sec?: number, ms?: number): number",
        "javascript.date.setMinutes",
        "number",
    ),
    (
        "setMonth",
        "(month: number, day?: number): number",
        "javascript.date.setMonth",
        "number",
    ),
    (
        "setSeconds",
        "(sec: number, ms?: number): number",
        "javascript.date.setSeconds",
        "number",
    ),
    (
        "setTime",
        "(time: number): number",
        "javascript.date.setTime",
        "number",
    ),
    (
        "toDateString",
        "(): string",
        "javascript.date.toDateString",
        "string",
    ),
    (
        "toISOString",
        "(): string",
        "javascript.date.toISOString",
        "string",
    ),
    ("toJSON", "(): string", "javascript.date.toJSON", "string"),
    (
        "toLocaleDateString",
        "(locale?: string, options?: object): string",
        "javascript.date.toLocaleDateString",
        "string",
    ),
    (
        "toLocaleString",
        "(locale?: string, options?: object): string",
        "javascript.date.toLocaleString",
        "string",
    ),
    (
        "toLocaleTimeString",
        "(locale?: string, options?: object): string",
        "javascript.date.toLocaleTimeString",
        "string",
    ),
    (
        "toString",
        "(): string",
        "javascript.date.toString",
        "string",
    ),
    (
        "toUTCString",
        "(): string",
        "javascript.date.toUTCString",
        "string",
    ),
    ("valueOf", "(): number", "javascript.date.valueOf", "number"),
];

pub const DATE_STATIC: &[Member] = &[
    (
        "UTC",
        "(year: number, month?: number, ...): number",
        "javascript.date_static.UTC",
        "number",
    ),
    ("now", "(): number", "javascript.date_static.now", "number"),
    (
        "parse",
        "(text: string): number",
        "javascript.date_static.parse",
        "number",
    ),
];

pub const REGEXP: &[Member] = &[
    (
        "exec",
        "(text: string): RegExpExecArray | null",
        "javascript.regexp.exec",
        "Array",
    ),
    ("flags", "string", "javascript.regexp.flags", "string"),
    ("global", "boolean", "javascript.regexp.global", "boolean"),
    (
        "lastIndex",
        "number",
        "javascript.regexp.lastIndex",
        "number",
    ),
    ("source", "string", "javascript.regexp.source", "string"),
    (
        "test",
        "(text: string): boolean",
        "javascript.regexp.test",
        "boolean",
    ),
    (
        "toString",
        "(): string",
        "javascript.regexp.toString",
        "string",
    ),
];

pub const ERROR: &[Member] = &[
    ("cause", "unknown", "javascript.error.cause", "any"),
    ("message", "string", "javascript.error.message", "string"),
    ("name", "string", "javascript.error.name", "string"),
    ("stack", "string", "javascript.error.stack", "string"),
];

pub const PROCESS: &[Member] = &[
    ("arch", "string", "javascript.process.arch", "string"),
    ("argv", "string[]", "javascript.process.argv", "Array"),
    ("cwd", "(): string", "javascript.process.cwd", "string"),
    (
        "env",
        "{ [key: string]: string | undefined }",
        "javascript.process.env",
        "Object",
    ),
    (
        "exit",
        "(code?: number): never",
        "javascript.process.exit",
        "void",
    ),
    (
        "hrtime",
        "(): [number, number]",
        "javascript.process.hrtime",
        "Array",
    ),
    (
        "memoryUsage",
        "(): object",
        "javascript.process.memoryUsage",
        "Object",
    ),
    (
        "nextTick",
        "(callback: Function, ...args: any[]): void",
        "javascript.process.nextTick",
        "void",
    ),
    (
        "on",
        "(event: string, listener: Function): process",
        "javascript.process.on",
        "any",
    ),
    ("pid", "number", "javascript.process.pid", "number"),
    (
        "platform",
        "string",
        "javascript.process.platform",
        "string",
    ),
    (
        "stderr",
        "WriteStream",
        "javascript.process.stderr",
        "Object",
    ),
    ("stdin", "ReadStream", "javascript.process.stdin", "Object"),
    (
        "stdout",
        "WriteStream",
        "javascript.process.stdout",
        "Object",
    ),
    (
        "uptime",
        "(): number",
        "javascript.process.uptime",
        "number",
    ),
    ("version", "string", "javascript.process.version", "string"),
    (
        "versions",
        "object",
        "javascript.process.versions",
        "Object",
    ),
];

pub const SYMBOL_STATIC: &[Member] = &[
    (
        "asyncIterator",
        "symbol",
        "javascript.symbol_static.asyncIterator",
        "symbol",
    ),
    (
        "for",
        "(key: string): symbol",
        "javascript.symbol_static.for",
        "symbol",
    ),
    (
        "iterator",
        "symbol",
        "javascript.symbol_static.iterator",
        "symbol",
    ),
    (
        "keyFor",
        "(symbol: symbol): string | undefined",
        "javascript.symbol_static.keyFor",
        "string",
    ),
    (
        "toPrimitive",
        "symbol",
        "javascript.symbol_static.toPrimitive",
        "symbol",
    ),
];

pub const URL_INSTANCE: &[Member] = &[
    ("hash", "string", "javascript.url_instance.hash", "string"),
    ("host", "string", "javascript.url_instance.host", "string"),
    (
        "hostname",
        "string",
        "javascript.url_instance.hostname",
        "string",
    ),
    ("href", "string", "javascript.url_instance.href", "string"),
    (
        "origin",
        "string",
        "javascript.url_instance.origin",
        "string",
    ),
    (
        "pathname",
        "string",
        "javascript.url_instance.pathname",
        "string",
    ),
    ("port", "string", "javascript.url_instance.port", "string"),
    (
        "protocol",
        "string",
        "javascript.url_instance.protocol",
        "string",
    ),
    (
        "search",
        "string",
        "javascript.url_instance.search",
        "string",
    ),
    (
        "searchParams",
        "URLSearchParams",
        "javascript.url_instance.searchParams",
        "Object",
    ),
    (
        "toString",
        "(): string",
        "javascript.url_instance.toString",
        "string",
    ),
];

/// Members of the instances of a built-in type, by name.
pub fn members(ty: &str) -> &'static [Member] {
    match ty {
        "String" => STRING,
        "Array" => ARRAY,
        "Number" => NUMBER,
        "Boolean" => BOOLEAN,
        "Function" => FUNCTION,
        "Object" => OBJECT,
        "Promise" => PROMISE,
        "Map" | "WeakMap" => MAP,
        "Set" | "WeakSet" => SET,
        "Date" => DATE,
        "RegExp" => REGEXP,
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" => ERROR,
        "URL" => URL_INSTANCE,
        _ => &[],
    }
}

/// Members of a global object or constructor (`Math.`, `Object.`, `console.`).
pub fn statics(name: &str) -> &'static [Member] {
    match name {
        "Math" => MATH,
        "JSON" => JSON,
        "console" => CONSOLE,
        "process" => PROCESS,
        "Object" => OBJECT_STATIC,
        "Array" => ARRAY_STATIC,
        "String" => STRING_STATIC,
        "Number" => NUMBER_STATIC,
        "Promise" => PROMISE_STATIC,
        "Date" => DATE_STATIC,
        "Symbol" => SYMBOL_STATIC,
        _ => &[],
    }
}

/// A global name: (name, kind, signature or type, documentation, built-in type of its value).
pub type Global = (
    &'static str,
    ItemKind,
    &'static str,
    &'static str,
    &'static str,
);

pub const GLOBALS: &[Global] = &[
    (
        "undefined",
        ItemKind::Constant,
        "undefined",
        "javascript.global.undefined",
        "",
    ),
    (
        "NaN",
        ItemKind::Constant,
        "number",
        "javascript.global.NaN",
        "Number",
    ),
    (
        "Infinity",
        ItemKind::Constant,
        "number",
        "javascript.global.Infinity",
        "Number",
    ),
    (
        "globalThis",
        ItemKind::Variable,
        "typeof globalThis",
        "javascript.global.globalThis",
        "",
    ),
    (
        "console",
        ItemKind::Variable,
        "Console",
        "javascript.global.console",
        "console",
    ),
    (
        "Math",
        ItemKind::Variable,
        "Math",
        "javascript.global.Math",
        "Math",
    ),
    (
        "JSON",
        ItemKind::Variable,
        "JSON",
        "javascript.global.JSON",
        "JSON",
    ),
    (
        "process",
        ItemKind::Variable,
        "NodeJS.Process",
        "javascript.global.process",
        "process",
    ),
    (
        "Object",
        ItemKind::Class,
        "ObjectConstructor",
        "javascript.global.Object",
        "Object",
    ),
    (
        "Array",
        ItemKind::Class,
        "ArrayConstructor",
        "javascript.global.Array",
        "Array",
    ),
    (
        "String",
        ItemKind::Class,
        "StringConstructor",
        "javascript.global.String",
        "String",
    ),
    (
        "Number",
        ItemKind::Class,
        "NumberConstructor",
        "javascript.global.Number",
        "Number",
    ),
    (
        "Boolean",
        ItemKind::Class,
        "BooleanConstructor",
        "javascript.global.Boolean",
        "Boolean",
    ),
    (
        "Symbol",
        ItemKind::Class,
        "SymbolConstructor",
        "javascript.global.Symbol",
        "Symbol",
    ),
    (
        "BigInt",
        ItemKind::Class,
        "BigIntConstructor",
        "javascript.global.BigInt",
        "",
    ),
    (
        "Promise",
        ItemKind::Class,
        "PromiseConstructor",
        "javascript.global.Promise",
        "Promise",
    ),
    (
        "Map",
        ItemKind::Class,
        "MapConstructor",
        "javascript.global.Map",
        "Map",
    ),
    (
        "Set",
        ItemKind::Class,
        "SetConstructor",
        "javascript.global.Set",
        "Set",
    ),
    (
        "WeakMap",
        ItemKind::Class,
        "WeakMapConstructor",
        "javascript.global.WeakMap",
        "WeakMap",
    ),
    (
        "WeakSet",
        ItemKind::Class,
        "WeakSetConstructor",
        "javascript.global.WeakSet",
        "WeakSet",
    ),
    (
        "Date",
        ItemKind::Class,
        "DateConstructor",
        "javascript.global.Date",
        "Date",
    ),
    (
        "RegExp",
        ItemKind::Class,
        "RegExpConstructor",
        "javascript.global.RegExp",
        "RegExp",
    ),
    (
        "Error",
        ItemKind::Class,
        "ErrorConstructor",
        "javascript.global.Error",
        "Error",
    ),
    (
        "TypeError",
        ItemKind::Class,
        "TypeErrorConstructor",
        "javascript.global.TypeError",
        "TypeError",
    ),
    (
        "RangeError",
        ItemKind::Class,
        "RangeErrorConstructor",
        "javascript.global.RangeError",
        "RangeError",
    ),
    (
        "SyntaxError",
        ItemKind::Class,
        "SyntaxErrorConstructor",
        "javascript.global.SyntaxError",
        "SyntaxError",
    ),
    (
        "ReferenceError",
        ItemKind::Class,
        "ReferenceErrorConstructor",
        "javascript.global.ReferenceError",
        "ReferenceError",
    ),
    (
        "Proxy",
        ItemKind::Class,
        "ProxyConstructor",
        "javascript.global.Proxy",
        "",
    ),
    (
        "Reflect",
        ItemKind::Variable,
        "Reflect",
        "javascript.global.Reflect",
        "",
    ),
    (
        "Intl",
        ItemKind::Variable,
        "Intl",
        "javascript.global.Intl",
        "",
    ),
    (
        "ArrayBuffer",
        ItemKind::Class,
        "ArrayBufferConstructor",
        "javascript.global.ArrayBuffer",
        "",
    ),
    (
        "Uint8Array",
        ItemKind::Class,
        "Uint8ArrayConstructor",
        "javascript.global.Uint8Array",
        "",
    ),
    (
        "Float64Array",
        ItemKind::Class,
        "Float64ArrayConstructor",
        "javascript.global.Float64Array",
        "",
    ),
    (
        "URL",
        ItemKind::Class,
        "typeof URL",
        "javascript.global.URL",
        "URL",
    ),
    (
        "URLSearchParams",
        ItemKind::Class,
        "typeof URLSearchParams",
        "javascript.global.URLSearchParams",
        "",
    ),
    (
        "TextEncoder",
        ItemKind::Class,
        "typeof TextEncoder",
        "javascript.global.TextEncoder",
        "",
    ),
    (
        "TextDecoder",
        ItemKind::Class,
        "typeof TextDecoder",
        "javascript.global.TextDecoder",
        "",
    ),
    (
        "AbortController",
        ItemKind::Class,
        "typeof AbortController",
        "javascript.global.AbortController",
        "",
    ),
    (
        "Buffer",
        ItemKind::Class,
        "typeof Buffer",
        "javascript.global.Buffer",
        "",
    ),
    (
        "parseInt",
        ItemKind::Function,
        "(text: string, radix?: number): number",
        "javascript.global.parseInt",
        "",
    ),
    (
        "parseFloat",
        ItemKind::Function,
        "(text: string): number",
        "javascript.global.parseFloat",
        "",
    ),
    (
        "isNaN",
        ItemKind::Function,
        "(value: any): boolean",
        "javascript.global.isNaN",
        "",
    ),
    (
        "isFinite",
        ItemKind::Function,
        "(value: any): boolean",
        "javascript.global.isFinite",
        "",
    ),
    (
        "encodeURIComponent",
        ItemKind::Function,
        "(text: string): string",
        "javascript.global.encodeURIComponent",
        "",
    ),
    (
        "decodeURIComponent",
        ItemKind::Function,
        "(text: string): string",
        "javascript.global.decodeURIComponent",
        "",
    ),
    (
        "encodeURI",
        ItemKind::Function,
        "(uri: string): string",
        "javascript.global.encodeURI",
        "",
    ),
    (
        "decodeURI",
        ItemKind::Function,
        "(uri: string): string",
        "javascript.global.decodeURI",
        "",
    ),
    (
        "setTimeout",
        ItemKind::Function,
        "(callback: Function, delay?: number, ...args: any[]): Timeout",
        "javascript.global.setTimeout",
        "",
    ),
    (
        "clearTimeout",
        ItemKind::Function,
        "(id: Timeout): void",
        "javascript.global.clearTimeout",
        "",
    ),
    (
        "setInterval",
        ItemKind::Function,
        "(callback: Function, delay?: number, ...args: any[]): Timeout",
        "javascript.global.setInterval",
        "",
    ),
    (
        "clearInterval",
        ItemKind::Function,
        "(id: Timeout): void",
        "javascript.global.clearInterval",
        "",
    ),
    (
        "setImmediate",
        ItemKind::Function,
        "(callback: Function, ...args: any[]): Immediate",
        "javascript.global.setImmediate",
        "",
    ),
    (
        "queueMicrotask",
        ItemKind::Function,
        "(callback: () => void): void",
        "javascript.global.queueMicrotask",
        "",
    ),
    (
        "structuredClone",
        ItemKind::Function,
        "(value: T): T",
        "javascript.global.structuredClone",
        "",
    ),
    (
        "fetch",
        ItemKind::Function,
        "(input: string | URL | Request, init?: object): Promise<Response>",
        "javascript.global.fetch",
        "Promise",
    ),
    (
        "atob",
        ItemKind::Function,
        "(data: string): string",
        "javascript.global.atob",
        "",
    ),
    (
        "btoa",
        ItemKind::Function,
        "(data: string): string",
        "javascript.global.btoa",
        "",
    ),
    (
        "require",
        ItemKind::Function,
        "(id: string): any",
        "javascript.global.require",
        "",
    ),
    (
        "module",
        ItemKind::Variable,
        "NodeModule",
        "javascript.global.module",
        "",
    ),
    (
        "exports",
        ItemKind::Variable,
        "any",
        "javascript.global.exports",
        "",
    ),
    (
        "__dirname",
        ItemKind::Constant,
        "string",
        "javascript.global.__dirname",
        "String",
    ),
    (
        "__filename",
        ItemKind::Constant,
        "string",
        "javascript.global.__filename",
        "String",
    ),
    (
        "window",
        ItemKind::Variable,
        "Window & typeof globalThis",
        "javascript.global.window",
        "",
    ),
    (
        "document",
        ItemKind::Variable,
        "Document",
        "javascript.global.document",
        "",
    ),
    (
        "navigator",
        ItemKind::Variable,
        "Navigator",
        "javascript.global.navigator",
        "",
    ),
    (
        "localStorage",
        ItemKind::Variable,
        "Storage",
        "javascript.global.localStorage",
        "",
    ),
    (
        "sessionStorage",
        ItemKind::Variable,
        "Storage",
        "javascript.global.sessionStorage",
        "",
    ),
    (
        "alert",
        ItemKind::Function,
        "(message?: any): void",
        "javascript.global.alert",
        "",
    ),
    (
        "requestAnimationFrame",
        ItemKind::Function,
        "(callback: (time: number) => void): number",
        "javascript.global.requestAnimationFrame",
        "",
    ),
];

pub const KEYWORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "of",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// Code templates: (trigger, label, text, caret offset in the text, description).
pub const SNIPPETS: &[(&str, &str, &str, usize, &str)] = &[
    ("log", "log", "console.log()", 12, "javascript.snippet.log"),
    (
        "clog",
        "clog",
        "console.log('', )",
        14,
        "javascript.snippet.clog",
    ),
    (
        "err",
        "err",
        "console.error()",
        14,
        "javascript.snippet.err",
    ),
    (
        "fori",
        "fori",
        "for (let i = 0; i < ; i++) {\n    \n}",
        22,
        "javascript.snippet.fori",
    ),
    (
        "forof",
        "forof",
        "for (const item of ) {\n    \n}",
        18,
        "javascript.snippet.forof",
    ),
    (
        "forin",
        "forin",
        "for (const key in ) {\n    \n}",
        17,
        "javascript.snippet.forin",
    ),
    ("if", "if", "if () {\n    \n}", 4, "javascript.snippet.if"),
    (
        "ife",
        "ife",
        "if () {\n    \n} else {\n    \n}",
        4,
        "javascript.snippet.ife",
    ),
    (
        "func",
        "func",
        "function name() {\n    \n}",
        9,
        "javascript.snippet.func",
    ),
    (
        "afunc",
        "afunc",
        "async function name() {\n    \n}",
        15,
        "javascript.snippet.afunc",
    ),
    (
        "arrow",
        "arrow",
        "const name = () => {\n    \n};",
        6,
        "javascript.snippet.arrow",
    ),
    (
        "try",
        "try",
        "try {\n    \n} catch (error) {\n    \n}",
        11,
        "javascript.snippet.try",
    ),
    (
        "class",
        "class",
        "class Name {\n    constructor() {\n        \n    }\n}",
        6,
        "javascript.snippet.class",
    ),
    (
        "imp",
        "imp",
        "import  from '';",
        7,
        "javascript.snippet.imp",
    ),
    (
        "req",
        "req",
        "const  = require('');",
        6,
        "javascript.snippet.req",
    ),
    (
        "exp",
        "exp",
        "export default ;",
        15,
        "javascript.snippet.exp",
    ),
    (
        "timeout",
        "timeout",
        "setTimeout(() => {\n    \n}, 1000);",
        23,
        "javascript.snippet.timeout",
    ),
    (
        "prom",
        "prom",
        "new Promise((resolve, reject) => {\n    \n})",
        38,
        "javascript.snippet.prom",
    ),
];

/// The completion items for names that need no declaration: keywords, snippets and globals.
pub fn global_items() -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::new();
    for (name, kind, detail, doc, _) in GLOBALS {
        let rank = if matches!(
            *name,
            "window"
                | "document"
                | "navigator"
                | "localStorage"
                | "sessionStorage"
                | "alert"
                | "requestAnimationFrame"
        ) {
            75
        } else {
            60
        };
        let mut item = CompletionItem::new(*name, *kind)
            .detail(*detail)
            .doc(crate::i18n::t(doc))
            .rank(rank);
        if *kind == ItemKind::Function {
            item = item.insert(format!("{name}()"), Some(name.len() + 1));
        }
        items.push(item);
    }
    for keyword in KEYWORDS {
        items.push(CompletionItem::new(*keyword, ItemKind::Keyword).rank(70));
    }
    for (_, label, text, caret, doc) in SNIPPETS {
        items.push(
            CompletionItem::new(*label, ItemKind::Snippet)
                .detail(crate::i18n::t(doc))
                .insert(*text, Some(*caret))
                .rank(80),
        );
    }
    items
}

/// The built-in type of a global (the key for [`statics`]), if it has members.
pub fn global_type(name: &str) -> Option<&'static str> {
    GLOBALS
        .iter()
        .find(|g| g.0 == name)
        .map(|g| g.4)
        .filter(|t| !t.is_empty())
}

pub fn global_doc(name: &str) -> Option<(&'static str, &'static str)> {
    GLOBALS
        .iter()
        .find(|g| g.0 == name)
        .map(|g| (g.2, crate::i18n::t(g.3)))
}

#[cfg(test)]
#[path = "../../../test/languages/javascript/builtins/tests.rs"]
mod tests;
