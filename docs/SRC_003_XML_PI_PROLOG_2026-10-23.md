# SRC-003 — XML declaration placement and processing instructions (partial)

This change tightens the bounded `XmlNestingGuard` public-dump envelope. An XML declaration is accepted only as the very first document construct, never after whitespace, comments, another processing instruction, or the root. Ordinary processing instructions with valid conservative ASCII XML names are accepted in the prolog, within element content, and after the document element, consistent with XML 1.0; empty, digit-prefixed, and malformed targets are rejected. The XML 1.0/UTF-8 declaration restrictions remain in force.

Regression tests exercise 1-, 2-, 7-, and 8192-byte decoder chunks for accepted and rejected cases. This is only an envelope grammar improvement: it does **not** qualify full XML namespaces/page events, authenticated upstream source completeness, independent page census, renderer fidelity, offline media, or real-device acceptance. SRC-003 stays unchecked pending those gates.
