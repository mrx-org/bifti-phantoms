function [data, affine] = readNifti(path)
%READNIFTI Read a 4-D NIfTI-1 file (.nii or .nii.gz) without any toolbox.
%   DATA is indexed (x, y, z, sub-volume) as stored, scaled by scl_slope /
%   scl_inter, as double (or complex double for complex data types). AFFINE
%   is the 3x4 sform voxel-to-world matrix (RAS+, mm) for zero-based indices.
    if numel(path) > 3 && strcmpi(path(end-2:end), '.gz')
        tmp = tempname;
        mkdir(tmp);
        cleanup = onCleanup(@() rmdir(tmp, 's'));
        path = char(gunzip(path, tmp));
    end
    fid = fopen(path, 'r', 'ieee-le');
    if fid < 0
        error('bifti:nifti', 'Cannot open NIfTI file %s', path);
    end
    closer = onCleanup(@() fclose(fid));
    if fread(fid, 1, 'int32') ~= 348
        % A big-endian file reads its header size byte-swapped. Replacing the
        % cleanup closes the little-endian handle.
        fid = fopen(path, 'r', 'ieee-be');
        closer = onCleanup(@() fclose(fid)); %#ok<NASGU>
        if fread(fid, 1, 'int32') ~= 348
            error('bifti:nifti', '%s is not a NIfTI-1 file', path);
        end
    end
    dim = header(fid, 40, 8, 'int16');
    datatype = header(fid, 70, 1, 'int16');
    voxOffset = header(fid, 108, 1, 'float32');
    slope = header(fid, 112, 1, 'float32');
    inter = header(fid, 116, 1, 'float32');
    affine = [header(fid, 280, 4, 'float32'), header(fid, 296, 4, 'float32'), header(fid, 312, 4, 'float32')]';
    if dim(1) ~= 4
        error('bifti:nifti', '%s must be 4-dimensional (NIFTI.md), got %d dimensions', path, dim(1));
    end
    shape = double(dim(2:5))';

    [precision, isComplex] = niftiType(datatype, path);
    fseek(fid, voxOffset, 'bof');
    count = prod(shape) * (1 + isComplex);
    raw = fread(fid, count, ['*' precision]);
    if numel(raw) ~= count
        error('bifti:nifti', '%s is truncated', path);
    end
    data = double(raw);
    if isComplex
        data = complex(data(1:2:end), data(2:2:end));
    end
    data = reshape(data, shape);
    if slope ~= 0
        data = data * slope + inter;
    end
end

function value = header(fid, offset, count, precision)
    fseek(fid, offset, 'bof');
    value = double(fread(fid, count, precision));
end

function [precision, isComplex] = niftiType(datatype, path)
    types = {2, 'uint8'; 4, 'int16'; 8, 'int32'; 16, 'single'; 32, 'single'; 64, 'double'; ...
        256, 'int8'; 512, 'uint16'; 768, 'uint32'; 1024, 'int64'; 1280, 'uint64'; 1792, 'double'};
    match = find([types{:, 1}] == datatype, 1);
    if isempty(match)
        error('bifti:nifti', '%s has unsupported NIfTI datatype %d', path, datatype);
    end
    precision = types{match, 2};
    isComplex = any(datatype == [32, 1792]);
end
