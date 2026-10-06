function writePhantom(path, config)
%WRITEPHANTOM Serialize a phantom config (see bifti.readPhantom) to a BIfTI JSON file.
%   bifti.writePhantom(path, config) creates the parent folder if needed. Properties
%   at their default value are omitted; unknown fields are written back.
%
%   See also bifti.readPhantom.
    json = bifti.internal.jsonObject('$schema', config.schema, ...
        'units', bifti.internal.defaultUnits(), ...
        'system', bifti.internal.jsonObject('gyro', config.system.gyro, 'B0', config.system.B0));
    if ~isempty(config.patient)
        json = addMember(json, 'patient', bifti.internal.jsonObject('position', config.patient));
    end
    if ~isempty(config.resliceTo)
        rows = num2cell(config.resliceTo.affine, 2)';
        json = addMember(json, 'reslice_to', bifti.internal.jsonObject( ...
            'affine', rows, 'resolution', config.resliceTo.resolution(:)'));
    end
    tissues = bifti.internal.jsonObject();
    for t = config.tissues(:)'
        tissues = addMember(tissues, t.name, tissueJson(t));
    end
    json = mergeMembers(addMember(json, 'tissues', tissues), config.unknown);

    folder = fileparts(path);
    if ~isempty(folder) && ~isfolder(folder)
        mkdir(folder);
    end
    fid = fopen(path, 'w');
    if fid < 0
        error('bifti:write', 'Cannot write %s', path);
    end
    closer = onCleanup(@() fclose(fid));
    fprintf(fid, '%s\n', bifti.internal.writeJson(json));
end

function json = tissueJson(t)
    json = bifti.internal.jsonObject('density', refString(t.density));
    defaults = {'T1', t.T1, Inf; 'T2', t.T2, Inf; 'T2''', t.T2dash, Inf; 'ADC', t.ADC, 0; 'dB0', t.dB0, 0};
    for k = 1:size(defaults, 1)
        if ~isequal(defaults{k, 2}, defaults{k, 3})
            json = addMember(json, defaults{k, 1}, propertyJson(defaults{k, 2}));
        end
    end
    channels = {'B1+', t.B1_tx; 'B1-', t.B1_rx};
    for k = 1:2
        if ~isequal(channels{k, 2}, {1})
            json = addMember(json, channels{k, 1}, cellfun(@propertyJson, channels{k, 2}, 'UniformOutput', false));
        end
    end
    json = mergeMembers(json, t.unknown);
end

function value = propertyJson(property)
    if isnumeric(property)
        value = property;
    elseif isfield(property, 'func')
        value = bifti.internal.jsonObject('file', refString(property), 'func', property.func);
    else
        value = refString(property);
    end
end

function s = refString(ref)
    s = sprintf('%s[%d]', ref.file, ref.index);
end

function object = addMember(object, key, value)
    object.keys{end + 1} = key;
    object.values{end + 1} = value;
end

function object = mergeMembers(object, other)
    object.keys = [object.keys, other.keys];
    object.values = [object.values, other.values];
end
