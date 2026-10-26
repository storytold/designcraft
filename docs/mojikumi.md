# Mojikumi repair scope

The retained changes address reproducible implementation defects: equal-priority
spacing allocation, discrete compression endpoints, leading-only compression,
spacing after complete shaped clusters, and tracking that made narrow punctuation
look full-width. Preflight also checks nested note and table-cell paragraph rules.

IDML import rejects malformed/non-finite numbers and integer coercion. Finite,
representable rules outside the composer's supported range remain preserved;
the rule resolver reports unsupported settings instead of rejecting the document.

The broad preset expansion, regional side-bearing changes, Tsume combination
policy, and CID preference/diagnostic extension were withdrawn after review.
The original supported preset set and Unicode classification remain in place.
No claim of full Adobe Mojikumi conformance or exact output parity is made.

Reference for priority semantics:
[Adobe spacing priorities](https://helpx.adobe.com/indesign/desktop/language-and-proofing/chinese-japanese-and-korean/set-spacing-priorities-in-mojikumi-character-classes.html).
