// 告诉 TypeScript：theme 下的所有 *.css 都是合法的副作用导入。
// 实际编译由 Rspack 处理，这里只为消除 TS 报错。
declare module '*.css';
