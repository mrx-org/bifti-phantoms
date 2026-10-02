function object = jsonObject(varargin)
%JSONOBJECT Ordered JSON object from key/value pairs, as parseJson returns it.
    object = struct('keys', {varargin(1:2:end)}, 'values', {varargin(2:2:end)});
end
