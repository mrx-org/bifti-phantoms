% BIFTI  Read, write and load BIfTI MRI simulation phantoms.
% Version 0.4.0 18-Nov-2026
%
% Phantom files
%   readPhantom          - Parse a phantom JSON (no NIfTI data).
%   writePhantom         - Write a phantom config back to JSON.
%   loadPhantom          - Load a phantom with all of its NIfTI maps.
%   savePhantom          - Save a loaded phantom as a folder of NIfTIs + JSON.
%   niftiFiles           - NIfTI files a phantom config references.
%
% Coordinates
%   scannerMatrix        - Phantom RAS+ to scanner rotation of a patient position.
%   scannerAffine        - Voxel-to-scanner affine of a tissue or affine.
%   placedAffine         - Affine of a reslice_to grid after its move_to.
%
% Public registry
%   loadCatalog          - Discovery list: label -> registry collection.
%   loadRegistry         - Every published collection.
%   flattenPhantoms      - Phantom JSON names of a collection.
%   loadRegistryPhantom  - Download a phantom and its NIfTIs into a cache.
%
% See https://github.com/mrx-org/bifti-phantoms for the format specification.
