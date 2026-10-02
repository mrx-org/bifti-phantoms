function phantom = loadPhantom(source, baseDir)
%LOADPHANTOM Load a BIfTI phantom with all of its NIfTI data.
%   phantom = bifti.loadPhantom(path) reads the phantom JSON and every NIfTI it
%   references (relative to the JSON's folder). phantom = bifti.loadPhantom(config,
%   baseDir) loads an already parsed, possibly edited, config (see bifti.readPhantom).
%
%   phantom.config is the parsed config; phantom.tissues is a struct array in
%   file order with the fields name, density, T1, T2, T2dash, ADC, dB0 (3-D
%   double arrays indexed (x, y, z)), B1_tx, B1_rx (cell arrays with one array
%   per channel, complex if the NIfTI data is complex) and affine (3x4
%   voxel-to-RAS+ matrix in mm for zero-based indices, as in NIfTI). Units are
%   those of JSON.md: s, 10^-3 mm^2/s, Hz, relative B1, density a fraction.
%
%   Every map of a tissue is brought onto one grid: reslice_to if the phantom
%   has one, otherwise the tissue's density grid. Resampling averages each
%   output voxel over the source region it covers, weighting intensive
%   properties by density (see the top-level README, "Reslicing"). func
%   mappings apply to the resampled values.
%
%   See also bifti.readPhantom, bifti.scannerAffine, bifti.loadRegistryPhantom.
    if ischar(source)
        config = bifti.readPhantom(source);
        baseDir = fileparts(absolutePath(source));
    else
        config = source;
    end
    % One NIfTI usually holds a property for several tissues: read each file once.
    cache = containers.Map();
    readRef = @(ref) readSubvolume(cache, baseDir, ref);

    tissues = struct('name', {}, 'density', {}, 'T1', {}, 'T2', {}, 'T2dash', {}, ...
        'ADC', {}, 'dB0', {}, 'B1_tx', {}, 'B1_rx', {}, 'affine', {});
    for k = 1:numel(config.tissues)
        tissues(k) = loadTissue(config.tissues(k), readRef, config.resliceTo);
    end
    phantom = struct('config', config, 'tissues', tissues);
end

function out = loadTissue(tissue, readRef, resliceTo)
    [densitySrc, srcAffine] = readRef(tissue.density);
    requireReal(densitySrc, tissue.name, 'density');
    target = resliceTo;
    if isempty(target)
        target = struct('affine', srcAffine, 'resolution', shape3(densitySrc));
    end
    grid.target = target;
    grid.srcAffine = srcAffine;
    grid.srcShape = shape3(densitySrc);
    grid.resampler = bifti.internal.resampler(srcAffine, grid.srcShape, target.affine, target.resolution);
    % Regularised so that a footprint without tissue averages its in-bounds taps
    % instead of 0/0; scaled because density need not be a [0, 1] float map.
    peak = max([densitySrc(:); 0]);
    grid.weight = densitySrc + max(1e-6 * peak, (peak == 0));
    grid.weightSum = bifti.internal.contract(grid.resampler, grid.weight);

    out.name = tissue.name;
    % density is extensive, so it is averaged unweighted: weighting it by itself
    % would remove exactly the partial-volume information resampling should give.
    out.density = bifti.internal.contract(grid.resampler, densitySrc);
    names = {'T1', 'T2', 'T2dash', 'ADC', 'dB0'};
    for n = names
        out.(n{1}) = requireReal(loadProperty(tissue.(n{1}), readRef, grid, n{1}), tissue.name, n{1});
    end
    out.B1_tx = loadChannels(tissue.B1_tx, readRef, grid, 'B1+');
    out.B1_rx = loadChannels(tissue.B1_rx, readRef, grid, 'B1-');
    out.affine = target.affine;
end

function maps = loadChannels(channels, readRef, grid, name)
    maps = cell(1, numel(channels));
    for c = 1:numel(channels)
        maps{c} = loadProperty(channels{c}, readRef, grid, sprintf('%s[%d]', name, c - 1));
    end
end

function data = loadProperty(property, readRef, grid, name)
    if isnumeric(property)
        data = repmat(property, grid.target.resolution);
        return
    end
    [native, affine] = readRef(property);
    if isequal(shape3(native), grid.srcShape) && norm(affine - grid.srcAffine) <= sqrt(eps) * norm(grid.srcAffine)
        if strcmp(grid.resampler.kind, 'identity')
            data = native;
        else
            numerator = bifti.internal.contract(grid.resampler, native .* grid.weight);
            data = numerator ./ grid.weightSum;
            data(grid.weightSum <= 0) = 0;
        end
    else
        % NIFTI.md requires all NIfTIs of a phantom to share one grid. A map that
        % does not cannot use the density weight and is averaged unweighted.
        warning('bifti:gridMismatch', ['%s does not share the density map''s grid; ' ...
            'resampling it unweighted, so its values may be diluted near edges'], name);
        r = bifti.internal.resampler(affine, shape3(native), grid.target.affine, grid.target.resolution);
        data = bifti.internal.contract(r, native);
    end
    if isfield(property, 'func')
        data = bifti.internal.applyFunc(property.func, data);
    end
end

function [data, affine] = readSubvolume(cache, baseDir, ref)
    path = ref.file;
    if ~isAbsolute(path)
        path = fullfile(baseDir, path);
    end
    if ~isKey(cache, path)
        [data, affine] = bifti.internal.readNifti(path);
        cache(path) = struct('data', data, 'affine', affine);
    end
    file = cache(path);
    count = size(file.data, 4);
    if ref.index < 0 || ref.index >= count
        error('bifti:reference', '%s has %d sub-volumes, cannot read [%d]', ref.file, count, ref.index);
    end
    data = file.data(:, :, :, ref.index + 1);
    affine = file.affine;
end

function data = requireReal(data, tissue, name)
    if ~isreal(data)
        error('bifti:complex', '%s of tissue "%s" must be real-valued', name, tissue);
    end
end

function s = shape3(x)
    s = [size(x, 1), size(x, 2), size(x, 3)];
end

function tf = isAbsolute(path)
    tf = any(path(1) == '/\') || ~isempty(regexp(path, '^[A-Za-z]:', 'once'));
end

function path = absolutePath(path)
    if ~isAbsolute(path)
        path = fullfile(pwd, path);
    end
end
