# CodeGraph

CodeGraph 面向不同编程语言的源码项目，向调用方提供统一的代码对象及关系。

## Language

**Language Analyzer（语言分析器）**:
可独立使用、理解一种或一组编程语言源码语义，并产出 Code Graph 的分析产品。
_Avoid_: Native Helper, 辅助程序, Narrant Helper

**Analyzer Package（分析器包）**:
可独立分发、具有明确版本的 Language Analyzer 及其所需组成部分。
_Avoid_: Helper 包, 项目依赖包

**Code Graph（代码图）**:
某个固定源码状态下、选定文件范围内的代码对象及其关系的表示。
_Avoid_: CodeGraph 项目, 语法树, 图差异

**Code Graph Difference（代码图差异）**:
两张 Code Graph 之间代码对象及关系的新增、删除和变化。
_Avoid_: Code Graph, 文本差异
