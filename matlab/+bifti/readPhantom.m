function config = readPhantom(path)
%READPHANTOM Parse a BIfTI phantom JSON file without loading any NIfTI data.
%   config = bifti.readPhantom(path) returns a struct with the fields
%     schema      the $schema URI
%     system      struct with gyro [MHz/T] and B0 [T]
%     patient     patient position code ('FFS', 'HFS', ...), '' if absent (= FFS)
%     resliceTo   [] or struct with affine (3x4) and resolution (1x3)
%     tissues     struct array in file order: name, density, T1, T2, T2dash,
%                 ADC, dB0, B1_tx, B1_rx, unknown
%     unknown     unrecognised top-level fields, kept so bifti.writePhantom round-trips them
%
%   A tissue property is a number, a NIfTI reference struct (file, index with
%   the zero-based sub-volume) or a transformed reference (file, index, func).
%   B1_tx/B1_rx are cell arrays with one property per channel. Omitted
%   properties take the defaults of JSON.md (T1, T2, T2dash Inf; ADC, dB0 0;
%   B1 {1}). Unknown fields are warned about and kept.
%
%   See also bifti.writePhantom, bifti.loadPhantom.
    json = bifti.internal.parseJson(fileread(path));
    config = bifti.internal.parseConfig(json);
end
