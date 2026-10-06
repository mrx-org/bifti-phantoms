function value = jsonGet(object, key, default)
%JSONGET Value of KEY in a JSON object from bifti.internal.parseJson.
%   Returns DEFAULT if the key is absent, or errors if no default is given.
    match = find(strcmp(object.keys, key), 1);
    if ~isempty(match)
        value = object.values{match};
    elseif nargin > 2
        value = default;
    else
        error('bifti:missingField', 'Missing required field "%s"', key);
    end
end
