# SRC-002 / PACK-002 category graph adapter — partial

Master SHA: `cd14d6a65badf579b0cbc5c4e86d1a762cf3790b`.
Rust CI passed: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38205801033
Platform CI passed: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38205801009

The adapter derives category membership and subcategory edges from revision-joined Enterprise records. It rejects missing categories, duplicate pages and mixed project/generation data; retains unresolved redirect aliases and requires explicit administrative classification. It does not authenticate official source bytes or establish complete snapshot coverage. SRC-002, PACK-002 and PACK-005 remain unchecked pending official-source and end-to-end qualification.
