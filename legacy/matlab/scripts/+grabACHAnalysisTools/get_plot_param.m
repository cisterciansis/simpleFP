function param_value = get_plot_param(plot_params_struct, param_name, default_value)
% GET_PLOT_PARAM Safely retrieves a parameter from a plot_params struct.
%   If the parameter 'param_name' exists in 'plot_params_struct' and is not
%   empty, its value is returned. Otherwise, 'default_value' is returned.

    if isfield(plot_params_struct, param_name) && ~isempty(plot_params_struct.(param_name))
        param_value = plot_params_struct.(param_name);
    else
        param_value = default_value;
    end
end