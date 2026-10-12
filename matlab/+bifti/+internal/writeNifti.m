function indices = writeNifti(path, volumes, affine)
%WRITENIFTI Write 3-D volumes as one 4-D NIfTI-1 file, deterministically.
%   indices = bifti.internal.writeNifti(path, volumes, affine) stacks the cell
%   array VOLUMES (3-D arrays of one shape) along the 4th dimension and returns
%   the zero-based index each has in the file. Volumes with identical stored
%   bytes share an index and are written once. Data is stored as double, or
%   complex double if any volume is complex. Orientation is the sform only
%   (sform_code 2, qform_code 0) from the 3x4 AFFINE, as NIFTI.md requires.
%   A path ending in .gz is compressed with a fixed gzip header (no mtime, no
%   file name), so equal input always gives equal bytes. The uncompressed
%   bytes match the Python, Rust and Julia writers.
    if isempty(volumes)
        error('bifti:write', '%s: no volumes to write', path);
    end
    shape = [size(volumes{1}, 1), size(volumes{1}, 2), size(volumes{1}, 3)];
    isComplex = any(cellfun(@(v) ~isreal(v), volumes));
    bigEndian = bigEndianHost();

    stored = {};
    indices = zeros(1, numel(volumes));
    for k = 1:numel(volumes)
        v = volumes{k};
        if ~isequal([size(v, 1), size(v, 2), size(v, 3)], shape) || ndims(v) > 3
            error('bifti:write', '%s: volumes differ in shape, cannot stack them', path);
        end
        values = double(v(:));
        if isComplex
            pair = [real(values).'; imag(values).'];
            values = pair(:);
        end
        if bigEndian
            values = swapbytes(values);
        end
        bytes = typecast(values, 'uint8');
        found = 0;
        for j = 1:numel(stored)
            if isequal(stored{j}, bytes)
                found = j;
                break
            end
        end
        if found == 0
            stored{end + 1} = bytes; %#ok<AGROW>
            found = numel(stored);
        end
        indices(k) = found - 1;
    end

    isGzip = numel(path) > 3 && strcmpi(path(end-2:end), '.gz');
    target = path;
    if isGzip
        scratch = tempname;
        mkdir(scratch);
        cleanup = onCleanup(@() rmdir(scratch, 's')); %#ok<NASGU>
        target = fullfile(scratch, 'volume.nii');
    end
    folder = fileparts(path);
    if ~isempty(folder) && ~isfolder(folder)
        mkdir(folder);
    end

    fid = fopen(target, 'w', 'ieee-le');
    if fid < 0
        error('bifti:write', 'Cannot write %s', path);
    end
    writeHeader(fid, shape, numel(stored), affine, isComplex);
    for j = 1:numel(stored)
        fwrite(fid, stored{j}, 'uint8');
    end
    fclose(fid);

    if isGzip
        gzip(target, scratch);
        blob = readBytes([target '.gz']);
        fid = fopen(path, 'w');
        if fid < 0
            error('bifti:write', 'Cannot write %s', path);
        end
        fwrite(fid, normalizeGzipHeader(blob), 'uint8');
        fclose(fid);
    end
end

function writeHeader(fid, shape, nVolumes, affine, isComplex)
% 352 bytes: the 348-byte header and the empty extension flag.
    if isComplex
        datatype = 1792; bitpix = 128;
    else
        datatype = 64; bitpix = 64;
    end
    voxelSizes = sqrt(sum(affine(1:3, 1:3) .^ 2, 1));
    fwrite(fid, 348, 'int32');                              % sizeof_hdr
    pad(fid, 36);                                           % through dim_info
    fwrite(fid, [4, shape, nVolumes, 1, 1, 1], 'int16');    % dim
    pad(fid, 14);                                           % intent_*
    fwrite(fid, [datatype, bitpix], 'int16');
    pad(fid, 2);                                            % slice_start
    fwrite(fid, [1, voxelSizes, 1], 'float32');             % pixdim: qfac, dx, dy, dz, dt
    pad(fid, 12);
    fwrite(fid, [352, 1], 'float32');                       % vox_offset, scl_slope
    pad(fid, 7);                                            % scl_inter, slice_end, slice_code
    fwrite(fid, 2, 'uint8');                                % xyzt_units: millimetres
    pad(fid, 128);
    fwrite(fid, [0, 2], 'int16');                           % qform_code, sform_code
    pad(fid, 24);                                           % quaternion and offset
    fwrite(fid, affine(1:3, 1:4).', 'float32');             % srow_x, srow_y, srow_z
    pad(fid, 16);                                           % intent_name
    fwrite(fid, uint8(['n+1' 0]), 'uint8');                 % magic
    pad(fid, 4);                                            % extension flag
end

function pad(fid, count)
    fwrite(fid, zeros(1, count), 'uint8');
end

function tf = bigEndianHost()
    [~, ~, endian] = computer();
    tf = endian == 'B';
end

function bytes = readBytes(path)
    fid = fopen(path, 'r');
    closer = onCleanup(@() fclose(fid)); %#ok<NASGU>
    bytes = fread(fid, Inf, '*uint8');
end

function out = normalizeGzipHeader(bytes)
% Replace the gzip header by a fixed one: no optional fields, mtime 0, XFL 0 and
% OS "unknown", whatever gzip() put there (file name, timestamp, host OS).
    bytes = bytes(:).';
    flags = bytes(4);
    at = 11;                                    % first byte after the fixed header
    if bitand(flags, 4)                         % FEXTRA
        at = at + 2 + double(bytes(at)) + 256 * double(bytes(at + 1));
    end
    if bitand(flags, 8)                         % FNAME
        at = at + find(bytes(at:end) == 0, 1);
    end
    if bitand(flags, 16)                        % FCOMMENT
        at = at + find(bytes(at:end) == 0, 1);
    end
    if bitand(flags, 2)                         % FHCRC
        at = at + 2;
    end
    out = [uint8([31 139 8 0 0 0 0 0 0 255]), bytes(at:end)];
end
