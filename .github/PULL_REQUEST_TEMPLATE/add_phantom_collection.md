## New phantom collection

<!-- Guide: ADD_PHANTOMS.md. Delete this comment block. -->

**Registry name:** `<author>-<name>-<number>`
**Catalog label:** 
**Zenodo record:** https://doi.org/10.5281/zenodo.XXXXXXX
**License:** 

**Description** (what is it, which tissues / field strengths / resolutions):

<!-- If this revises an existing collection, name the old entry: -->
**Replaces / revises:** none

### Checklist

- [ ] All phantom JSONs and all NIfTIs they reference are in the **same, published** Zenodo record
- [ ] `doi` is the **version DOI** (`10.5281/zenodo.<id>`), not the concept DOI
- [ ] Every filename in `phantoms` exists in the record (or in its single `configs.tar`, not both)
- [ ] One new entry added to `registry.json`; no existing entry modified or removed
- [ ] Matching label added (or re-pointed) in `catalog.json`
- [ ] Phantoms load with one of the implementations
- [ ] Ran `tools/validate_registry.py`, `tools/validate_catalog.py` and `tools/check_registry_immutable.py` locally
- [ ] Authors, license and attribution are correct for the data (including any source-data license terms)
